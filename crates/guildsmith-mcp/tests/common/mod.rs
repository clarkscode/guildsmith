//! Shared helpers for MCP protocol tests of the tool groups: an in-process client, calls,
//! annotation checks (contracts/mcp-tools.md) and output-schema validation.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::fake::FakeDiscord;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig, Tool};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct Client;

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

pub type Conn = RunningService<RoleClient, Client>;

/// Annotations: read-only, destructive, idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ann {
    /// RO (and idempotent).
    Ro,
    /// W, not idempotent.
    W,
    /// W, idempotent.
    Wi,
    /// D, not idempotent.
    D,
    /// D, idempotent.
    Di,
}

impl Ann {
    fn hints(self) -> (bool, bool, bool) {
        match self {
            Ann::Ro => (true, false, true),
            Ann::W => (false, false, false),
            Ann::Wi => (false, false, true),
            Ann::D => (false, true, false),
            Ann::Di => (false, true, true),
        }
    }
}

pub async fn connect(app: Arc<App<FakeDiscord>>) -> Conn {
    let (server_io, client_io) = tokio::io::duplex(1 << 22);
    let server = GuildsmithServer::new(app);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    Client.serve(client_io).await.expect("client connects")
}

/// An app over `fake` with `groups` registered (plus core).
pub fn app(fake: FakeDiscord, groups: &[Toolset], policy: Policy) -> Arc<App<FakeDiscord>> {
    let mut toolsets: BTreeSet<Toolset> = groups.iter().copied().collect();
    toolsets.insert(Toolset::Core);
    Arc::new(App::new(fake, Policy { toolsets, ..policy }))
}

pub async fn call(c: &Conn, name: &str, args: Value) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name.to_owned());
    params.arguments = Some(args.as_object().expect("object arguments").clone());
    c.call_tool(params).await.unwrap_or_else(|e| panic!("{name}: {e}"))
}

pub fn texts(r: &CallToolResult) -> Vec<String> {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

pub fn tool<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name} not listed"))
}

/// Every tool of `group` is listed, in order, with the expected annotations, an output schema
/// and a one-sentence description.
pub async fn check_group(c: &Conn, group: Toolset, expected: &[(&str, Ann)]) -> Vec<Tool> {
    let tools = c.list_all_tools().await.unwrap();
    let names: Vec<&str> = expected.iter().map(|(n, _)| *n).collect();
    assert_eq!(group.tools(), names.as_slice(), "the contract's tool list for {group}");
    for (name, ann) in expected {
        let def = tool(&tools, name);
        let hints = def.annotations.as_ref().unwrap_or_else(|| panic!("{name}: annotations"));
        let (read_only, destructive, idempotent) = ann.hints();
        assert_eq!(hints.read_only_hint, Some(read_only), "{name} readOnlyHint");
        assert_eq!(hints.destructive_hint, Some(destructive), "{name} destructiveHint");
        assert_eq!(hints.idempotent_hint, Some(idempotent), "{name} idempotentHint");
        assert_eq!(hints.open_world_hint, Some(true), "{name} openWorldHint");
        assert!(def.output_schema.is_some(), "{name} outputSchema");
        let desc = def.description.as_deref().unwrap_or_default();
        assert!(!desc.is_empty() && desc.len() < 200, "{name}: one short sentence: {desc}");
    }
    tools
}

/// `structuredContent` of a successful result matches the tool's output schema.
pub fn assert_valid(tools: &[Tool], name: &str, r: &CallToolResult) {
    assert_eq!(r.is_error, Some(false), "{name}: {:?}", texts(r));
    let t = tool(tools, name);
    let schema = Value::Object(t.output_schema.as_ref().expect("outputSchema").as_ref().clone());
    let value = r.structured_content.clone().unwrap_or(Value::Null);
    if let Err(e) = jsonschema::validate(&schema, &value) {
        panic!("{name} output doesn't match its outputSchema: {e}\n{value:#}");
    }
}

/// The refusal kind of a refused call.
pub fn refusal_kind(r: &CallToolResult) -> String {
    assert_eq!(r.is_error, Some(true), "{:?}", texts(r));
    r.structured_content.as_ref().and_then(|v| v["refusal"]["kind"].as_str()).unwrap_or_default().to_owned()
}

/// The text blocks wrap user content: the untrusted heading, and no raw `marker` outside it.
pub fn assert_wrapped(r: &CallToolResult, marker: &str) {
    let all = texts(r).join("\n");
    assert!(all.contains("Untrusted content from Discord users"), "{all}");
    let outside = all.split("<discord-data>").next().unwrap_or_default();
    assert!(!outside.contains(marker), "user content outside the wrapper: {all}");
}
