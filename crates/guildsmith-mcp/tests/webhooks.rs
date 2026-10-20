//! T067: the `webhooks` group over MCP: the 5 tools with their annotations, outputs that match
//! their schemas, the URL only in `webhook_create`'s result, and never in a list.

mod common;

use common::{Ann, assert_valid, call, check_group, connect, texts};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use serde_json::json;

const TOOLS: &[(&str, Ann)] = &[
    ("webhook_create", Ann::W),
    ("webhooks_list", Ann::Ro),
    ("webhook_update", Ann::Wi),
    ("webhook_delete", Ann::D),
    ("webhook_send", Ann::W),
];

#[tokio::test]
async fn webhooks_over_mcp_keep_the_url_secret() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Webhooks], Policy::default());
    let c = connect(app.clone()).await;
    let tools = check_group(&c, Toolset::Webhooks, TOOLS).await;
    let g = "1000";
    let created = call(&c, "webhook_create", json!({"guild": g, "channel": "#general", "name": "CI"})).await;
    assert_valid(&tools, "webhook_create", &created);
    let url = created.structured_content.as_ref().unwrap()["secrets"]["url"].as_str().unwrap().to_owned();
    let token = url.rsplit('/').next().unwrap().to_owned();
    assert!(texts(&created)[0].contains("secret"), "the summary says it's a secret");
    for (name, args) in [
        ("webhooks_list", json!({"guild": g})),
        ("webhook_send", json!({"guild": g, "webhook": "CI", "content": "deployed", "username": "Deploys"})),
        ("webhook_update", json!({"guild": g, "webhook": "CI", "name": "CI 2"})),
        ("webhook_delete", json!({"guild": g, "webhook": "CI 2"})),
    ] {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
        let all = format!("{} {}", texts(&r).join(" "), r.structured_content.unwrap_or_default());
        assert!(!all.contains(&token) && !all.contains("/api/webhooks/"), "{name} leaked: {all}");
    }
}
