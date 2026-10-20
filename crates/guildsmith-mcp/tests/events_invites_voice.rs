//! T047: the `events`, `invites` and `voice` groups over MCP: the 13 tools with their
//! annotations, outputs that match their schemas, and user-written names wrapped.

mod common;

use common::{Ann, assert_valid, assert_wrapped, call, check_group, connect};
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use serde_json::json;

const EVENTS: &[(&str, Ann)] = &[
    ("event_create", Ann::W),
    ("event_update", Ann::Wi),
    ("event_delete", Ann::D),
    ("events_list", Ann::Ro),
    ("event_users", Ann::Ro),
];
const INVITES: &[(&str, Ann)] =
    &[("invite_create", Ann::W), ("invites_list", Ann::Ro), ("invite_revoke", Ann::D), ("invite_target_users", Ann::W)];
const VOICE: &[(&str, Ann)] =
    &[("voice_members", Ann::W), ("voice_state", Ann::Ro), ("stage", Ann::W), ("voice_channel_status", Ann::Wi)];
const GROUPS: &[Toolset] = &[Toolset::Events, Toolset::Invites, Toolset::Voice];

fn rich() -> FakeDiscord {
    FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())])
}

#[tokio::test]
async fn all_13_tools_are_listed_with_their_annotations() {
    let c = connect(common::app(rich(), GROUPS, Policy::default())).await;
    check_group(&c, Toolset::Events, EVENTS).await;
    check_group(&c, Toolset::Invites, INVITES).await;
    let tools = check_group(&c, Toolset::Voice, VOICE).await;
    assert_eq!(tools.len(), 23 + 13);
}

#[tokio::test]
async fn outputs_match_their_schemas() {
    let app = common::app(rich(), GROUPS, Policy::default());
    app.port.join_voice(fixtures::GUILD, fixtures::SAM, fixtures::LOBBY);
    let c = connect(app.clone()).await;
    let tools = c.list_all_tools().await.unwrap();
    let g = "1000";
    let ev = call(
        &c,
        "event_create",
        json!({"guild": g, "name": "Game night", "start": "2030-10-23T20:00",
        "location": {"voice": "Lobby"}, "recurrence": {"frequency": "weekly", "weekday": ["friday"]}}),
    )
    .await;
    assert_valid(&tools, "event_create", &ev);
    assert_wrapped(&ev, "Game night");
    let inv =
        call(&c, "invite_create", json!({"guild": g, "channel": "#general", "max_age": 3600, "max_uses": 5})).await;
    assert_valid(&tools, "invite_create", &inv);
    let code = inv.structured_content.as_ref().unwrap()["code"].as_str().unwrap().to_owned();
    for (name, args) in [
        ("event_update", json!({"guild": g, "event": "Game night", "description": "Bring snacks"})),
        ("event_update", json!({"guild": g, "event": "Game night", "status": "canceled"})),
        ("events_list", json!({"guild": g})),
        ("event_users", json!({"guild": g, "event": "Game night"})),
        ("event_delete", json!({"guild": g, "event": "Game night"})),
        ("invites_list", json!({"guild": g})),
        ("invite_target_users", json!({"guild": g, "code": code, "action": "list"})),
        ("invite_revoke", json!({"guild": g, "code": code})),
        ("voice_members", json!({"guild": g, "members": ["sam", "alex"], "mute": true})),
        ("voice_state", json!({"guild": g, "member": "sam"})),
        ("stage", json!({"guild": g, "channel": "Town Hall", "action": "start", "topic": "AMA"})),
        ("stage", json!({"guild": g, "channel": "Town Hall", "action": "end"})),
        ("voice_channel_status", json!({"guild": g, "channel": "Lobby", "status": "Chess"})),
    ] {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
    }
    let vm = call(&c, "voice_members", json!({"guild": g, "members": ["alex"], "disconnect": true})).await;
    let sc = vm.structured_content.unwrap();
    assert!(sc["failed"][0]["reason"].as_str().unwrap().contains("not in a voice channel"), "{sc}");
}
