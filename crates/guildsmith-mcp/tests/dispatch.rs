//! T082: the dispatcher (research E1, US8). The default `tools/list` has 23 tools; `toolsets`
//! lists, describes and enables groups; `tool_call` reaches unlocked groups with the same
//! policy as a direct call; `tools/list` never changes.

use std::collections::BTreeSet;
use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
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

async fn connect(app: Arc<App<FakeDiscord>>) -> RunningService<RoleClient, Client> {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = GuildsmithServer::new(app);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    Client.serve(client_io).await.expect("client connects")
}

fn app(policy: Policy) -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), policy))
}

async fn call(c: &RunningService<RoleClient, Client>, name: &'static str, args: Value) -> CallToolResult {
    c.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())).await.expect("call")
}

async fn names(c: &RunningService<RoleClient, Client>) -> Vec<String> {
    c.list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect()
}

#[tokio::test]
async fn the_default_list_is_23_tools_and_never_changes() {
    let a = app(Policy::default());
    let c = connect(a.clone()).await;
    let before = names(&c).await;
    assert_eq!(before.len(), 23);
    assert!(before.contains(&"toolsets".to_owned()) && before.contains(&"tool_call".to_owned()));
    let res = call(&c, "toolsets", json!({"action": "enable", "toolset": "moderation"})).await;
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    assert_eq!(res.structured_content.unwrap()["unlocked"], true);
    assert_eq!(names(&c).await, before, "enabling doesn't change tools/list");
    assert!(a.toolsets.is_unlocked(Toolset::Moderation));
}

#[tokio::test]
async fn list_reports_every_group_with_counts_tokens_permissions_and_state() {
    let policy = Policy {
        toolsets: BTreeSet::from([Toolset::Core, Toolset::Moderation]),
        blocked: BTreeSet::from([Toolset::Raw]),
        ..Policy::default()
    };
    let c = connect(app(policy)).await;
    let res = call(&c, "toolsets", json!({"action": "list"})).await;
    let sc = res.structured_content.unwrap();
    let rows = sc["toolsets"].as_array().unwrap();
    assert_eq!(rows.len(), 13);
    let row = |n: &str| rows.iter().find(|r| r["name"] == n).unwrap().clone();
    let core = row("core");
    assert_eq!(core["tools"], 23);
    assert_eq!((core["registered"].clone(), core["unlocked"].clone()), (json!(true), json!(true)));
    let m = row("moderation");
    assert_eq!(m["registered"], true);
    assert!(m["approx_tokens"].as_u64().unwrap() > 0);
    assert!(m["required_permissions"].as_array().unwrap().contains(&json!("ban_members")));
    let polls = row("polls");
    assert_eq!(
        (polls["registered"].clone(), polls["unlocked"].clone(), polls["blocked"].clone()),
        (json!(false), json!(false), json!(false))
    );
    assert_eq!(polls["tools"], 3);
    assert_eq!(row("raw")["blocked"], true);
    assert_eq!(row("messages")["required_intents"], json!(["MESSAGE_CONTENT"]));
    for r in rows {
        for key in
            ["name", "purpose", "tools", "approx_tokens", "required_permissions", "registered", "unlocked", "blocked"]
        {
            assert!(r.get(key).is_some(), "{key} missing in {r}");
        }
    }
}

#[tokio::test]
async fn describe_returns_the_groups_tool_definitions() {
    let c = connect(app(Policy::default())).await;
    let res = call(&c, "toolsets", json!({"action": "describe", "toolset": "moderation"})).await;
    let sc = res.structured_content.unwrap();
    let tools = sc["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 10, "the moderation group's tools");
    for t in tools {
        assert!(t["name"].is_string() && t["description"].is_string(), "{t}");
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["annotations"]["destructiveHint"].is_boolean(), "{t}");
    }
    let bad = call(&c, "toolsets", json!({"action": "describe", "toolset": "gateway"})).await;
    assert_eq!(bad.is_error, Some(true));
}

#[tokio::test]
async fn a_blocked_group_cant_be_enabled_or_called() {
    let a = app(Policy { blocked: BTreeSet::from([Toolset::Moderation]), ..Policy::default() });
    let c = connect(a.clone()).await;
    let res = call(&c, "toolsets", json!({"action": "enable", "toolset": "moderation"})).await;
    assert_eq!(res.is_error, Some(true));
    assert_eq!(res.structured_content.unwrap()["refusal"]["kind"], "blocked");
    let res =
        call(&c, "tool_call", json!({"tool": "member_kick", "arguments": {"guild": "1000", "member": "sam"}})).await;
    assert_eq!(res.structured_content.unwrap()["refusal"]["kind"], "blocked");
    assert!(!a.toolsets.is_unlocked(Toolset::Moderation));
    assert_eq!(a.port.writes(), []);
}

#[tokio::test]
async fn tool_call_to_a_locked_group_is_refused_and_direct_calls_need_listing() {
    let a = app(Policy::default());
    let c = connect(a.clone()).await;
    let res =
        call(&c, "tool_call", json!({"tool": "member_kick", "arguments": {"guild": "1000", "member": "sam"}})).await;
    let r = res.structured_content.unwrap()["refusal"].clone();
    assert_eq!(r["kind"], "locked");
    assert!(r["hint"].as_str().unwrap().contains("enable"), "{r}");
    // Calling an unlisted tool directly is an unknown tool, with a pointer to the dispatcher.
    let direct = c
        .call_tool(
            CallToolRequestParams::new("member_kick")
                .with_arguments(json!({"member": "sam"}).as_object().unwrap().clone()),
        )
        .await;
    let err = direct.expect_err("not listed").to_string();
    assert!(err.contains("tool_call"), "{err}");
    let nested = call(&c, "tool_call", json!({"tool": "tool_call", "arguments": {}})).await;
    assert_eq!(nested.is_error, Some(true));
    assert_eq!(a.port.writes(), []);
}

#[tokio::test]
async fn tool_call_applies_the_same_policy_as_a_direct_call() {
    let a = app(Policy::default());
    let c = connect(a.clone()).await;
    call(&c, "toolsets", json!({"action": "enable", "toolset": "moderation"})).await;
    // Destructive without confirm: a preview, nothing changed.
    let res =
        call(&c, "tool_call", json!({"tool": "member_kick", "arguments": {"guild": "1000", "member": "sam"}})).await;
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    assert_eq!(res.structured_content.unwrap()["preview"], true);
    assert_eq!(a.port.writes(), []);
    // With confirm: done, with an audit reason.
    let args = json!({"guild": "1000", "member": "sam", "confirm": true});
    let res = call(&c, "tool_call", json!({"tool": "member_kick", "arguments": args})).await;
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    let w = a.port.writes();
    assert_eq!(w.len(), 1);
    assert!(w[0].reason.as_deref().unwrap().starts_with("guildsmith: "));

    // Read-only mode refuses through the dispatcher too, before any request.
    let ro = app(Policy { read_only: true, ..Policy::default() });
    let c = connect(ro.clone()).await;
    call(&c, "toolsets", json!({"action": "enable", "toolset": "moderation"})).await;
    let args = json!({"guild": "1000", "member": "sam", "confirm": true});
    let res = call(&c, "tool_call", json!({"tool": "member_kick", "arguments": args})).await;
    assert_eq!(res.structured_content.unwrap()["refusal"]["kind"], "read_only");
    assert_eq!(ro.port.writes(), []);
}
