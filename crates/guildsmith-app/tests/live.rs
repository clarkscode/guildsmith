//! T076: the live services against `FakeDiscord`.

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::live::{self, ChannelInput, RoleInput};
use guildsmith_core::Permission;
use guildsmith_core::snapshot::{LiveChannelKind, OverwriteKind};
use guildsmith_core::spec::ChannelKind;

const G: Option<&str> = Some("1000");

fn app() -> App<FakeDiscord> {
    App::new(fixtures::lab_fake(), Policy::default())
}

fn assert_audited(app: &App<FakeDiscord>) {
    for w in app.port.writes() {
        if w.method == "send_message" {
            continue; // messages have no audit log
        }
        let reason = w.reason.unwrap_or_default();
        assert!(reason.starts_with("guildsmith: "), "{}: {reason}", w.method);
    }
}

#[tokio::test]
async fn role_upsert_creates_without_role_and_updates_with_one() {
    let app = app();
    let input =
        RoleInput { name: Some("Helper".to_owned()), color: Some("#00FF00".to_owned()), ..RoleInput::default() };
    let created = live::role_upsert(&app, G, None, &input, Some("onboarding")).await.unwrap();
    assert!(created.created);
    let role = app.port.current(fixtures::GUILD).unwrap().role(created.id).cloned().unwrap();
    assert_eq!((role.name.as_str(), role.color), ("Helper", 0x00_FF_00));
    assert_eq!(app.port.writes()[0].reason.as_deref(), Some("guildsmith: create role @Helper (onboarding)"));

    let update = RoleInput { hoist: Some(true), color: Some("#00FF00".to_owned()), ..RoleInput::default() };
    let updated = live::role_upsert(&app, G, Some("@Helper"), &update, None).await.unwrap();
    assert!(!updated.created);
    assert_eq!(updated.changes, ["hoist false → true"], "only what differs");
    let again = live::role_upsert(&app, G, Some("@Helper"), &update, None).await.unwrap();
    assert_eq!(again.changes, Vec::<String>::new(), "idempotent: nothing to send");
    assert_eq!(app.port.writes().len(), 2);
    assert_audited(&app);
}

#[tokio::test]
async fn channel_upsert_places_a_channel_under_a_parent() {
    let app = app();
    let input = ChannelInput {
        name: Some("announcements".to_owned()),
        kind: Some("text".to_owned()),
        parent: Some("Community".to_owned()),
        topic: Some("News".to_owned()),
        ..ChannelInput::default()
    };
    let up = live::channel_upsert(&app, G, None, &input, None).await.unwrap();
    let ch = app.port.current(fixtures::GUILD).unwrap().channel(up.id).cloned().unwrap();
    assert_eq!(ch.parent_id, Some(fixtures::COMMUNITY));
    assert_eq!(ch.kind, LiveChannelKind::Known(ChannelKind::Text));

    // Move it to the top level by name.
    let moved = ChannelInput { parent: Some("none".to_owned()), ..ChannelInput::default() };
    let up = live::channel_upsert(&app, G, Some("#announcements"), &moved, None).await.unwrap();
    assert_eq!(up.changes, ["category Community → none"]);
    assert_eq!(app.port.current(fixtures::GUILD).unwrap().channel(up.id).unwrap().parent_id, None);
    assert_audited(&app);
}

#[tokio::test]
async fn channels_reorder_sends_one_request() {
    let app = app();
    let order = ["#mod-log".to_owned(), "#general".to_owned()];
    let moved = live::channels_reorder(&app, G, &order, None, None).await.unwrap();
    assert_eq!(moved.moved, 2);
    let writes = app.port.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].method, "reorder_channels");
    let s = app.port.current(fixtures::GUILD).unwrap();
    assert!(s.channel(fixtures::MOD_LOG).unwrap().position < s.channel(fixtures::GENERAL).unwrap().position);
    assert_audited(&app);
}

#[tokio::test]
async fn permissions_set_and_clear() {
    let app = app();
    let allow = ["view_channel".to_owned()];
    let set = live::permissions_set(&app, G, "#general", "@Member", &allow, &[], false, None).await.unwrap();
    assert_eq!(set.overwrite.unwrap().target, "@Member");
    let general = app.port.current(fixtures::GUILD).unwrap().channel(fixtures::GENERAL).cloned().unwrap();
    let ow = general.overwrites.iter().find(|o| o.id == fixtures::MEMBER_ROLE).unwrap();
    assert!(ow.allow.contains(Permission::ViewChannel));

    // A member target, then clearing the role's overwrite.
    live::permissions_set(&app, G, "#general", "member:51", &[], &allow, false, None).await.unwrap();
    let cleared = live::permissions_set(&app, G, "#general", "@Member", &[], &[], true, None).await.unwrap();
    assert!(cleared.overwrite.is_none());
    let general = app.port.current(fixtures::GUILD).unwrap().channel(fixtures::GENERAL).cloned().unwrap();
    assert!(!general.overwrites.iter().any(|o| o.id == fixtures::MEMBER_ROLE));
    assert!(general.overwrites.iter().any(|o| o.id == fixtures::SAM && o.kind == OverwriteKind::Member));
    assert_audited(&app);
}

#[tokio::test]
async fn permissions_explain_names_the_deciding_overwrite() {
    let app = app();
    let e = live::permissions_explain(&app, G, "#mod-log", "@everyone", Some("view_channel")).await.unwrap();
    assert!(!e.allowed.contains(Permission::ViewChannel));
    assert_eq!(e.verdict.as_deref(), Some("view_channel is denied; decided by overwrite for @everyone"));
    let e = live::permissions_explain(&app, G, "#mod-log", "alex", Some("view_channel")).await.unwrap();
    assert!(e.allowed.contains(Permission::ViewChannel), "alex is a moderator");
}

#[tokio::test]
async fn member_roles_adds_and_removes() {
    let app = app();
    let out =
        live::member_roles(&app, G, "@sam", &["Moderator".to_owned()], &["@Member".to_owned()], None).await.unwrap();
    assert_eq!(out.roles, ["@Moderator"]);
    assert_eq!(app.port.writes().len(), 2);
    // Nothing left to change: no requests.
    live::member_roles(&app, G, "51", &["Moderator".to_owned()], &[], None).await.unwrap();
    assert_eq!(app.port.writes().len(), 2);
    assert_audited(&app);
}

#[tokio::test]
async fn messages_read_defaults_to_20_and_caps_at_100() {
    let app = app();
    let sam = fixtures::lab_members().into_iter().find(|m| m.id == fixtures::SAM).unwrap();
    for i in 0..120 {
        app.port.post_as_user(fixtures::GUILD, fixtures::GENERAL, &sam, &format!("message {i}"));
    }
    assert_eq!(live::messages_read(&app, G, "#general", None, None).await.unwrap().len(), 20);
    assert_eq!(live::messages_read(&app, G, "#general", Some(255), None).await.unwrap().len(), 100);
    let sent = live::message_send(&app, G, "#general", "hello").await.unwrap();
    assert!(sent.url.ends_with(&format!("/1000/101/{}", sent.id)), "{}", sent.url);
    let newest = live::messages_read(&app, G, "general", Some(1), None).await.unwrap();
    assert_eq!(newest[0].content, "hello");
}

#[tokio::test]
async fn deletes_need_confirm_and_never_touch_everyone() {
    let app = app();
    let preview = live::role_delete(&app, G, "@Member", false, None).await.unwrap();
    assert!(preview.is_preview());
    assert_eq!(app.port.writes(), []);
    let done = live::role_delete(&app, G, "@Member", true, None).await.unwrap();
    assert!(matches!(done, Outcome::Done(_)));
    let err = live::role_delete(&app, G, "@everyone", true, None).await.unwrap_err();
    assert!(matches!(err, ServiceError::Invalid(_)), "{err:?}");
    let preview = live::channel_delete(&app, G, "Community", false, None).await.unwrap();
    let Outcome::Preview(p) = preview else { panic!() };
    assert!(p.would.iter().any(|w| w.contains("2 channel(s) move to the top level")), "{:?}", p.would);
    assert_audited(&app);
}
