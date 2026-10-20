//! T060: the `automod` group over MCP: the 3 tools with their annotations, outputs that match
//! their schemas, and the TOML-like trigger/action shapes as JSON input.

mod common;

use common::{Ann, assert_valid, call, check_group, connect, refusal_kind};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use serde_json::json;

const TOOLS: &[(&str, Ann)] =
    &[("automod_rules_list", Ann::Ro), ("automod_rule_upsert", Ann::Wi), ("automod_rule_delete", Ann::D)];

#[tokio::test]
async fn rules_round_trip_over_mcp() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Automod], Policy::default());
    let c = connect(app.clone()).await;
    let tools = check_group(&c, Toolset::Automod, TOOLS).await;
    let g = "1000";
    let created = call(
        &c,
        "automod_rule_upsert",
        json!({"guild": g, "name": "No invite links",
        "trigger": {"keyword": {"regex": ["discord\\.gg/\\w+"]}},
        "actions": [{"block": {"message": "Invites aren't allowed here."}}, {"alert": {"channel": "#mod-log"}}],
        "exempt_channels": ["#general"]}),
    )
    .await;
    assert_valid(&tools, "automod_rule_upsert", &created);
    let spam = call(
        &c,
        "automod_rule_upsert",
        json!({"guild": g, "name": "Spam", "trigger": "spam", "actions": [{"block": {}}]}),
    )
    .await;
    assert_valid(&tools, "automod_rule_upsert", &spam);
    let mentions = call(&c, "automod_rule_upsert", json!({"guild": g, "name": "Pings",
        "trigger": {"mention_spam": {"limit": 10, "raid_protection": true}}, "actions": [{"timeout": {"duration": 3600}}]})).await;
    assert_valid(&tools, "automod_rule_upsert", &mentions);
    let list = call(&c, "automod_rules_list", json!({"guild": g})).await;
    assert_valid(&tools, "automod_rules_list", &list);
    assert_eq!(list.structured_content.as_ref().unwrap()["rules"].as_array().unwrap().len(), 3);
    let off = call(&c, "automod_rule_upsert", json!({"guild": g, "rule": "Spam", "enabled": false})).await;
    assert_valid(&tools, "automod_rule_upsert", &off);
    assert_eq!(off.structured_content.unwrap()["preview"], true);
    let del = call(&c, "automod_rule_delete", json!({"guild": g, "rule": "Pings"})).await;
    assert_valid(&tools, "automod_rule_delete", &del);
    let bad = call(
        &c,
        "automod_rule_upsert",
        json!({"guild": g, "name": "Too many", "trigger": "spam",
        "actions": [{"timeout": {"duration": 60}}]}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    assert!(bad.structured_content.unwrap()["error"].as_str().unwrap().contains("only allowed on keyword"));

    let ro = common::app(fixtures::lab_fake(), &[Toolset::Automod], Policy { read_only: true, ..Policy::default() });
    let c = connect(ro.clone()).await;
    let r = call(
        &c,
        "automod_rule_upsert",
        json!({"guild": g, "name": "x", "trigger": "spam", "actions": [{"block": {}}]}),
    )
    .await;
    assert_eq!(refusal_kind(&r), "read_only");
}
