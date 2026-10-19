//! T052: op shapes — one update per channel (R7), category sync, kind changes (R10), moves and
//! positions (R8), request estimates (R9).

mod common;

use common::{lab, perms};
use guildsmith_core::planner::{OpKind, Plan, Ref, Risk};
use guildsmith_core::snapshot::{LiveChannelKind, OverwriteKind, Snapshot};
use guildsmith_core::spec::{ChannelKind, parse};
use guildsmith_core::{Id, Permission, diff};

fn plan(snapshot: &Snapshot, toml: &str) -> Plan {
    diff(&parse(toml).expect("spec parses"), snapshot).unwrap_or_else(|e| panic!("{e}"))
}

const COMMUNITY: &str = "[[categories]]\nname = \"Community\"\n";

#[test]
fn overwrite_and_field_changes_share_one_update() {
    let toml = format!(
        "[[roles]]\nname = \"Moderator\"\n{COMMUNITY}[[categories.channels]]\nname = \"general\"\nkind = \"text\"\ntopic = \"new\"\n\
         overwrites = [{{ target = \"@Moderator\", allow = [\"manage_messages\"] }}]\n"
    );
    let p = plan(&lab(), &toml);
    assert_eq!(p.ops.len(), 1, "{:?}", p.ops);
    let OpKind::UpdateChannel { id, attrs, overwrites, changes, .. } = &p.ops[0].kind else { panic!("{:?}", p.ops[0]) };
    assert_eq!(*id, Id(101));
    assert_eq!(attrs.topic.as_deref(), Some("new"));
    let list = overwrites.as_ref().expect("full overwrite list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].target, Ref::Id(Id(10)));
    assert_eq!(changes.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(), ["topic", "overwrites"]);
}

#[test]
fn sync_permissions_copies_the_category_overwrites_explicitly() {
    let toml = r#"
[[categories]]
name = "Community"
overwrites = [{ target = "@everyone", deny = ["send_messages"] }]
  [[categories.channels]]
  name = "general"
  kind = "text"
  sync_permissions = true
"#;
    let p = plan(&lab(), toml);
    let general = p.ops.iter().find(|o| o.target == Ref::Id(Id(101))).expect("general is updated");
    let OpKind::UpdateChannel { overwrites: Some(list), .. } = &general.kind else { panic!("{general:?}") };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].target, Ref::Id(common::GUILD));
    assert_eq!(list[0].deny, perms(&[Permission::SendMessages]));
}

#[test]
fn text_to_announcement_changes_in_place_only_with_news() {
    let toml = format!("{COMMUNITY}[[categories.channels]]\nname = \"general\"\nkind = \"announcement\"\n");
    let mut s = lab();
    s.guild.features.insert("NEWS".to_owned());
    let p = plan(&s, &toml);
    assert_eq!(p.ops.len(), 1);
    let OpKind::UpdateChannel { attrs, .. } = &p.ops[0].kind else { panic!() };
    assert_eq!(attrs.kind, Some(LiveChannelKind::Known(ChannelKind::Announcement)));
    assert_eq!(p.risk, Risk::Safe);

    // Without NEWS it's a destructive replace, which needs opt-in.
    let err = diff(&parse(&toml).unwrap(), &lab()).unwrap_err();
    assert!(err.to_string().contains("set `replace = true`"), "{err}");
}

#[test]
fn other_kind_changes_are_replaces_that_need_opt_in() {
    let toml = format!("{COMMUNITY}[[categories.channels]]\nname = \"general\"\nkind = \"voice\"\nreplace = true\n");
    let p = plan(&lab(), &toml);
    assert_eq!(p.risk, Risk::Destructive);
    let group: Vec<_> = p.ops.iter().filter(|o| o.replace_group.is_some()).collect();
    assert_eq!(group.len(), 2, "{:?}", p.ops);
    assert_eq!(group[0].replace_group, group[1].replace_group);
    assert!(matches!(group[0].kind, OpKind::CreateChannel { .. }));
    assert!(matches!(group[1].kind, OpKind::DeleteChannel { id: Id(101) }));

    // prune.channels also allows it.
    let toml =
        format!("[prune]\nchannels = true\n{COMMUNITY}[[categories.channels]]\nname = \"general\"\nkind = \"voice\"\n");
    assert!(plan(&lab(), &toml).ops.iter().any(|o| o.replace_group.is_some()));
}

#[test]
fn parent_changes_go_in_the_channel_update_and_reorders_set_positions_only() {
    // Move #general (by id: names only match within their scope) to the top level, above Lobby.
    let toml = "[[channels]]\nid = \"101\"\nname = \"general\"\nkind = \"text\"\n[[channels]]\nname = \"Lobby\"\nkind = \"voice\"\n";
    let p = plan(&lab(), toml);
    let update = p.ops.iter().find(|o| o.target == Ref::Id(Id(101))).expect("general moves");
    let OpKind::UpdateChannel { parent, changes, .. } = &update.kind else { panic!() };
    assert_eq!(*parent, Some(None), "moved to the top level in its own update");
    assert!(changes.iter().any(|c| c.field == "category"));
    let reorder = p.ops.iter().find(|o| matches!(o.kind, OpKind::ReorderChannels { .. })).expect("one reorder");
    let OpKind::ReorderChannels { scopes } = &reorder.kind else { unreachable!() };
    // Positions only: the reorder carries orders, never parents.
    let json = serde_json::to_string(scopes).unwrap();
    assert!(!json.contains("parent"), "{json}");
}

#[test]
fn estimates_requests_and_warns_on_many_role_creates() {
    let toml = (0..51).map(|i| format!("[[roles]]\nname = \"r{i}\"\n")).collect::<Vec<_>>().concat();
    let p = plan(&lab(), &toml);
    let creates = p.ops.iter().filter(|o| matches!(o.kind, OpKind::CreateRole { .. })).count();
    assert_eq!(creates, 51);
    // One per op, three snapshot reads, and three more for the reorder's fresh view.
    assert_eq!(p.estimated_requests as usize, p.ops.len() + 3 + 3);
    assert!(p.warnings.iter().any(|w| w.contains("creates 51 roles")), "{:?}", p.warnings);
}

#[test]
fn creates_reference_new_roles_by_key() {
    let toml = r#"
[[roles]]
name = "Staff"
[[categories]]
name = "Community"
  [[categories.channels]]
  name = "staff-room"
  kind = "text"
  overwrites = [{ target = "@everyone", deny = ["view_channel"] }, { target = "@Staff", allow = ["view_channel"] }]
"#;
    let p = plan(&lab(), toml);
    assert_eq!(p.ops[0].summary, "create role @Staff");
    let create = p.ops.iter().find(|o| o.summary == "create text channel #staff-room in Community").expect("create");
    let OpKind::CreateChannel { parent, overwrites: Some(list), .. } = &create.kind else { panic!() };
    assert_eq!(*parent, Some(Ref::Id(Id(100))));
    assert!(list.iter().any(|o| o.target == Ref::New("role:Staff".to_owned()) && o.kind == OverwriteKind::Role));
}
