//! T024: the `moderation` group over MCP: the 10 tools with their annotations, outputs that
//! match their schemas, previews without `confirm`, and read-only refusals before any request.

mod common;

use common::{Ann, assert_valid, assert_wrapped, call, check_group, connect, refusal_kind};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use serde_json::json;

const TOOLS: &[(&str, Ann)] = &[
    ("member_ban", Ann::D),
    ("ban_remove", Ann::W),
    ("bans_list", Ann::Ro),
    ("member_timeout", Ann::Wi),
    ("member_kick", Ann::D),
    ("members_prune", Ann::D),
    ("messages_purge", Ann::D),
    ("roles_bulk_assign", Ann::W),
    ("audit_log", Ann::Ro),
    ("raid_lockdown", Ann::Di),
];

#[tokio::test]
async fn all_ten_tools_are_listed_with_their_annotations() {
    let c = connect(common::app(fixtures::lab_fake(), &[Toolset::Moderation], Policy::default())).await;
    let tools = check_group(&c, Toolset::Moderation, TOOLS).await;
    assert_eq!(tools.len(), 23 + 10);
}

#[tokio::test]
async fn outputs_match_their_schemas_and_destructive_tools_preview() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Moderation], Policy::default());
    let c = connect(app.clone()).await;
    let tools = c.list_all_tools().await.unwrap();
    let g = "1000";
    let previews = [
        ("member_ban", json!({"guild": g, "members": ["sam", "alex"], "delete_message_seconds": 3600})),
        ("member_kick", json!({"guild": g, "member": "sam"})),
        ("members_prune", json!({"guild": g, "days": 30})),
        ("messages_purge", json!({"guild": g, "channel": "#general"})),
        ("raid_lockdown", json!({"guild": g, "pause_invites_for": 3600})),
    ];
    for (name, args) in previews {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
        assert_eq!(r.structured_content.as_ref().unwrap()["preview"], true, "{name}");
    }
    assert_eq!(app.port.writes(), [], "previews change nothing");

    let runs = [
        ("member_timeout", json!({"guild": g, "members": ["sam"], "duration": 60})),
        ("roles_bulk_assign", json!({"guild": g, "role": "@Member", "add": ["alex"]})),
        ("member_ban", json!({"guild": g, "member": "sam", "confirm": true})),
        ("bans_list", json!({"guild": g})),
        ("ban_remove", json!({"guild": g, "user": "51"})),
        ("audit_log", json!({"guild": g})),
        ("raid_lockdown", json!({"guild": g, "clear": true, "confirm": true})),
    ];
    for (name, args) in runs {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
    }
    let done = call(&c, "member_kick", json!({"guild": g, "member": "alex", "confirm": true})).await;
    assert_valid(&tools, "member_kick", &done);
    assert_eq!(done.structured_content.unwrap()["succeeded"][0]["name"], "Lexi");
}

#[tokio::test]
async fn member_names_and_ban_reasons_are_wrapped_as_untrusted() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Moderation], Policy::default());
    let c = connect(app).await;
    let r = call(&c, "member_kick", json!({"guild": "1000", "member": "alex"})).await;
    assert_wrapped(&r, "Lexi");
    call(&c, "member_ban", json!({"guild": "1000", "member": "sam", "confirm": true, "reason": "spam"})).await;
    let bans = call(&c, "bans_list", json!({"guild": "1000"})).await;
    assert_wrapped(&bans, "\"reason\"");
}

#[tokio::test]
async fn read_only_refuses_every_write_before_any_request() {
    let app =
        common::app(fixtures::lab_fake(), &[Toolset::Moderation], Policy { read_only: true, ..Policy::default() });
    let c = connect(app.clone()).await;
    let g = "1000";
    for (name, args) in [
        ("member_ban", json!({"guild": g, "member": "sam", "confirm": true})),
        ("ban_remove", json!({"guild": g, "user": "51"})),
        ("member_timeout", json!({"guild": g, "member": "sam", "duration": 60})),
        ("member_kick", json!({"guild": g, "member": "sam", "confirm": true})),
        ("members_prune", json!({"guild": g, "days": 7, "confirm": true})),
        ("messages_purge", json!({"guild": g, "channel": "#general", "confirm": true})),
        ("roles_bulk_assign", json!({"guild": g, "role": "@Member", "add": ["alex"]})),
        ("raid_lockdown", json!({"guild": g, "clear": true, "confirm": true})),
    ] {
        let reads = app.port.reads();
        assert_eq!(refusal_kind(&call(&c, name, args).await), "read_only", "{name}");
        assert_eq!(app.port.reads(), reads, "{name} refused before any request");
    }
    assert_eq!(app.port.writes(), []);
    let r = call(&c, "bans_list", json!({"guild": g})).await;
    assert_eq!(r.is_error, Some(false), "reads still work");
}
