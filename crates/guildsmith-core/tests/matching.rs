//! T051: matching (research R12) and the visibility guard (R6).

mod common;

use common::{GUILD, lab, perms, role, text};
use guildsmith_core::planner::{OpKind, Plan};
use guildsmith_core::snapshot::{Completeness, RoleTags, Snapshot};
use guildsmith_core::spec::parse;
use guildsmith_core::{Id, Permission, PlanError, diff};

fn plan(snapshot: &Snapshot, toml: &str) -> Result<Plan, PlanError> {
    diff(&parse(toml).expect("spec parses"), snapshot)
}

fn summaries(p: &Plan) -> Vec<&str> {
    p.ops.iter().map(|o| o.summary.as_str()).collect()
}

#[test]
fn matches_by_id_first_so_renames_are_updates() {
    let p = plan(&lab(), "[[roles]]\nid = \"10\"\nname = \"Mods\"\n").unwrap();
    assert_eq!(summaries(&p), ["update role @Moderator: name \"Moderator\" → \"Mods\""]);
}

#[test]
fn matches_by_unique_name_in_scope() {
    // Same names as the live server, no ids: nothing to do.
    let toml = r#"
[[roles]]
name = "Moderator"
[[categories]]
name = "Community"
  [[categories.channels]]
  name = "general"
  kind = "text"
"#;
    assert!(plan(&lab(), toml).unwrap().is_empty());
}

#[test]
fn same_channel_name_in_two_categories_does_not_conflict() {
    let mut s = lab();
    s.channels.push(common::channel(200, "Staff", guildsmith_core::snapshot::LiveChannelKind::Category, None, 5));
    s.channels.push(text(201, "general", Some(200), 0));
    let toml = r#"
[[categories]]
name = "Community"
  [[categories.channels]]
  name = "general"
  kind = "text"
[[categories]]
name = "Staff"
  [[categories.channels]]
  name = "general"
  kind = "text"
  topic = "staff only"
"#;
    let p = plan(&s, toml).unwrap();
    assert_eq!(summaries(&p), ["update channel #general: topic \"\" → \"staff only\""]);
    assert!(matches!(p.ops[0].kind, OpKind::UpdateChannel { id: Id(201), .. }));
}

#[test]
fn ambiguous_names_are_errors_that_name_the_candidates() {
    let mut s = lab();
    s.channels.push(text(104, "general", Some(100), 5));
    let err = plan(
        &s,
        "[[categories]]\nname = \"Community\"\n[[categories.channels]]\nname = \"general\"\nkind = \"text\"\n",
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("more than one") && msg.contains("(101)") && msg.contains("(104)"), "{msg}");
}

#[test]
fn everyone_can_be_updated_but_not_created_deleted_or_moved() {
    let p = plan(&lab(), "[prune]\nroles = true\n[[roles]]\nname = \"@everyone\"\npermissions = [\"view_channel\"]\n")
        .unwrap();
    assert!(p.ops.iter().any(|o| matches!(o.kind, OpKind::UpdateRole { id, .. } if id == GUILD)));
    for op in &p.ops {
        assert!(!matches!(op.kind, OpKind::CreateRole { .. }), "{}", op.summary);
        assert!(!matches!(op.kind, OpKind::DeleteRole { id } if id == GUILD), "{}", op.summary);
        if let OpKind::ReorderRoles { order } = &op.kind {
            assert!(order.iter().all(|r| r.role != guildsmith_core::planner::Ref::Id(GUILD)));
        }
    }
}

fn with_managed_role(mut s: Snapshot) -> Snapshot {
    let mut bot = role(9, "Guildsmith", 3, perms(&[Permission::Administrator]));
    bot.managed = true;
    bot.tags = Some(RoleTags { bot_id: Some(Id(2)), integration_id: None, premium_subscriber: false });
    s.roles.push(bot);
    s
}

#[test]
fn managed_roles_need_managed_true_and_then_only_permissions_change() {
    let s = with_managed_role(lab());
    let err = plan(&s, "[[roles]]\nname = \"Guildsmith\"\npermissions = []\n").unwrap_err();
    assert!(err.to_string().contains("mark it `managed = true`"), "{err}");

    let p = plan(
        &s,
        "[[roles]]\nname = \"Guildsmith\"\nmanaged = true\npermissions = [\"administrator\", \"kick_members\"]\n",
    )
    .unwrap();
    assert_eq!(summaries(&p), ["update role @Guildsmith: permissions"]);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);

    // Anything but permissions is flagged; preflight refuses it before apply.
    let p = plan(&s, "[[roles]]\nname = \"Guildsmith\"\nmanaged = true\nhoist = true\n").unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("only its permissions can change")), "{:?}", p.warnings);

    let err = plan(&lab(), "[[roles]]\nname = \"Moderator\"\nmanaged = true\n").unwrap_err();
    assert!(err.to_string().contains("no managed role"), "{err}");
}

#[test]
fn hidden_id_is_a_hard_error_without_administrator() {
    let mut s = lab();
    s.completeness = Completeness::MaybeIncomplete;
    s.channels.retain(|c| c.id != Id(102)); // #mod-log is hidden from the bot
    let toml = "[[categories]]\nname = \"Community\"\n[[categories.channels]]\nid = \"102\"\nname = \"mod-log\"\nkind = \"text\"\n";
    let err = plan(&s, toml).unwrap_err();
    assert!(err.to_string().contains("missing or hidden from the bot"), "{err}");
}

#[test]
fn missing_id_with_administrator_recreates_with_a_stale_id_warning() {
    let mut s = lab();
    s.channels.retain(|c| c.id != Id(102)); // #mod-log was deleted
    let toml = "[[categories]]\nname = \"Community\"\n[[categories.channels]]\nid = \"102\"\nname = \"mod-log\"\nkind = \"text\"\n";
    let p = plan(&s, toml).unwrap();
    assert_eq!(summaries(&p), ["create text channel #mod-log in Community"]);
    assert!(p.warnings.iter().any(|w| w.contains("stale id")), "{:?}", p.warnings);
}

#[test]
fn unmanaged_items_are_deleted_only_with_that_kinds_prune_flag() {
    // An empty spec: everything live is unmanaged.
    let p = plan(&lab(), "").unwrap();
    assert!(p.is_empty());
    assert_eq!(p.unmanaged.len(), 6, "{:?}", p.unmanaged); // 2 roles, 1 category, 3 channels

    let p = plan(&lab(), "[prune]\nchannels = true\n").unwrap();
    assert!(p.ops.iter().all(|o| matches!(o.kind, OpKind::DeleteChannel { .. })));
    assert_eq!(p.ops.len(), 3, "{:?}", summaries(&p));

    let p = plan(&lab(), "[prune]\nroles = true\n").unwrap();
    // @Moderator is the bot's own top role (position 2): only @Member can go.
    assert_eq!(summaries(&p), ["delete role @Member (11)"]);
    assert!(p.warnings.iter().any(|w| w.contains("not pruning role @Moderator")), "{:?}", p.warnings);
}
