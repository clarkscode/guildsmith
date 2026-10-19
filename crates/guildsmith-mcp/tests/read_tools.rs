//! T042: MCP protocol tests for the US1 read tools, over an in-process client.

use std::collections::BTreeSet;
use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_core::Id;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig, Tool};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
struct Client;

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

const READ_TOOLS: &[&str] =
    &["guilds_list", "guild_overview", "roles_list", "channels_tree", "spec_export", "spec_validate"];

async fn connect(fake: FakeDiscord, policy: Policy) -> RunningService<RoleClient, Client> {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = GuildsmithServer::new(Arc::new(App::new(fake, policy)));
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    Client.serve(client_io).await.expect("client connects")
}

fn lab() -> FakeDiscord {
    FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())])
}

fn allow(ids: &[u64]) -> Policy {
    Policy { allowlist: Some(ids.iter().map(|i| Id(*i)).collect::<BTreeSet<_>>()), ..Policy::default() }
}

async fn call(c: &RunningService<RoleClient, Client>, name: &'static str, args: Value) -> CallToolResult {
    c.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())).await.expect("call")
}

fn tool<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("tool {name} not listed"))
}

fn assert_matches_schema(t: &Tool, result: &CallToolResult) {
    assert_eq!(result.is_error, Some(false), "{} failed: {:?}", t.name, result.content);
    let schema = Value::Object(t.output_schema.as_ref().expect("outputSchema").as_ref().clone());
    let value = result.structured_content.as_ref().expect("structuredContent");
    if let Err(e) = jsonschema::validate(&schema, value) {
        panic!("{} output doesn't match its outputSchema: {e}\n{value:#}", t.name);
    }
}

#[tokio::test]
async fn read_tools_are_listed_read_only_with_schemas() {
    let c = connect(lab(), Policy::default()).await;
    let tools = c.list_all_tools().await.unwrap();
    for name in READ_TOOLS {
        let t = tool(&tools, name);
        let a = t.annotations.as_ref().expect("annotations");
        assert_eq!(a.read_only_hint, Some(true), "{name}");
        assert_eq!(a.open_world_hint, Some(true), "{name}");
        assert!(t.output_schema.is_some(), "{name} has an outputSchema");
        let desc = t.description.as_deref().unwrap_or_default();
        assert!(!desc.is_empty() && desc.matches(". ").count() == 0, "{name}: one sentence: {desc}");
    }
}

#[tokio::test]
async fn results_match_their_output_schemas() {
    let c = connect(lab(), Policy::default()).await;
    let tools = c.list_all_tools().await.unwrap();
    let cases = [
        ("guilds_list", json!({})),
        ("guild_overview", json!({"guild": "1000"})),
        ("roles_list", json!({"guild": "1000", "fields": ["permissions", "color"], "limit": 2})),
        ("channels_tree", json!({"guild": "Guildsmith Lab", "fields": ["topic"]})),
        ("spec_export", json!({"guild": "1000"})),
        ("spec_validate", json!({"toml": "[[roles]]\nname = \"x\"\n"})),
    ];
    for (name, args) in cases {
        let res = call(&c, name, args).await;
        assert_matches_schema(tool(&tools, name), &res);
    }
    // Paging: two roles, then a cursor.
    let page = call(&c, "roles_list", json!({"guild": "1000", "limit": 2})).await.structured_content.unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(page["next_cursor"].is_string());
    assert!(page["items"][0].get("permissions").is_none(), "key fields only by default");
}

#[tokio::test]
async fn spec_export_never_overwrites() {
    let c = connect(lab(), Policy::default()).await;
    let path = std::env::temp_dir().join(format!("guildsmith-export-{}.toml", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let first = call(&c, "spec_export", json!({"guild": "1000", "path": path.to_str().unwrap()})).await;
    assert_eq!(first.is_error, Some(false));
    assert!(std::fs::read_to_string(&path).unwrap().contains("[guild]"));
    let second = call(&c, "spec_export", json!({"guild": "1000", "path": path.to_str().unwrap()})).await;
    assert_eq!(second.is_error, Some(true));
    assert!(second.structured_content.unwrap()["error"].as_str().unwrap().contains("refusing to overwrite"));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn servers_outside_the_allowlist_are_refused() {
    let c = connect(lab(), allow(&[999])).await;
    let res = call(&c, "guild_overview", json!({"guild": "1000"})).await;
    assert_eq!(res.is_error, Some(true));
    let sc = res.structured_content.unwrap();
    assert_eq!(sc["refusal"]["kind"], "not_allowed");
    let list = call(&c, "guilds_list", json!({})).await.structured_content.unwrap();
    assert_eq!(list["guilds"], json!([]), "servers outside the allowlist are hidden");
}

#[tokio::test]
async fn guild_may_be_omitted_with_exactly_one_allowed_server() {
    let c = connect(lab(), allow(&[1000])).await;
    let res = call(&c, "guild_overview", json!({})).await;
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    assert_eq!(res.structured_content.unwrap()["guild"]["name"], "Guildsmith Lab");
}

#[tokio::test]
async fn removed_bot_says_so() {
    let c = connect(lab(), Policy::default()).await;
    let res = call(&c, "guild_overview", json!({"guild": "2000"})).await;
    assert_eq!(res.is_error, Some(true));
    let msg = res.structured_content.unwrap()["error"].as_str().unwrap().to_owned();
    assert!(msg.contains("bot is not in this server"), "{msg}");
}

#[tokio::test]
async fn no_servers_hint_explains_bots_cant_create_servers() {
    let c = connect(FakeDiscord::new(vec![]), Policy::default()).await;
    let res = call(&c, "guilds_list", json!({})).await;
    let sc = res.structured_content.unwrap();
    assert_eq!(sc["hint"], "invite the bot to a server you created; bots can't create servers");
}

#[tokio::test]
async fn spec_validate_reports_line_and_column() {
    let c = connect(lab(), Policy::default()).await;
    let sc = call(&c, "spec_validate", json!({"toml": "[[roles]]\nnmae = \"x\"\n"})).await.structured_content.unwrap();
    assert_eq!(sc["valid"], false);
    assert_eq!(sc["problems"][0]["location"], json!({"line": 2, "col": 1}));
    assert_eq!(sc["problems"][0]["hint"], "did you mean `name`?");
}
