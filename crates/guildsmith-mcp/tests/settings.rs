//! T081: the `settings` group over MCP: the 7 tools with their annotations and outputs that
//! match their schemas.

mod common;

use common::{Ann, assert_valid, call, check_group, connect, refusal_kind};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use serde_json::json;

const TOOLS: &[(&str, Ann)] = &[
    ("settings_get", Ann::Ro),
    ("settings_update", Ann::Di),
    ("onboarding_update", Ann::Di),
    ("welcome_screen_update", Ann::Wi),
    ("templates", Ann::W),
    ("template_import", Ann::Ro),
    ("integration_remove", Ann::D),
];

#[tokio::test]
async fn settings_over_mcp() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Settings], Policy::default());
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/../guildsmith-core/tests/fixtures/templates/study.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    app.port.add_public_template("2TfF7a6xQFwT", "Study Group", fixture["serialized_source_guild"].clone());
    let c = connect(app.clone()).await;
    let tools = check_group(&c, Toolset::Settings, TOOLS).await;
    let g = "1000";
    for (name, args) in [
        ("settings_get", json!({"guild": g})),
        ("settings_update", json!({"guild": g, "description": "Lab", "afk_timeout": 900})),
        ("settings_update", json!({"guild": g, "description": "Lab", "afk_timeout": 900, "confirm": true})),
        ("onboarding_update", json!({"guild": g, "enabled": false, "prompts": []})),
        (
            "welcome_screen_update",
            json!({"guild": g, "enabled": false, "channels": [{"channel": "#general", "description": "Say hi"}]}),
        ),
        ("templates", json!({"guild": g, "action": "create", "name": "Lab"})),
        ("templates", json!({"guild": g, "action": "list"})),
        ("template_import", json!({"code_or_url": "https://discord.com/template/2TfF7a6xQFwT"})),
    ] {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
    }
    let use_it = call(&c, "templates", json!({"guild": g, "action": "use"})).await;
    assert_eq!(refusal_kind(&use_it), "unsupported");
}
