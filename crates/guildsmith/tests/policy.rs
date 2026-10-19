//! T067: the same policy for both interfaces (constitution I, SC-007): allowlist refusals,
//! read-only mode refusing every write before any request, and the empty-allowlist warning.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::{Cli, Exit};
use guildsmith::{dispatch_with, startup_warning};
use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, Toolset};
use guildsmith_core::Id;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{CallToolRequestParams, ClientConfig};
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

const SPEC: &str = "[[roles]]\nname = \"Member\"\nhoist = true\n";
const HASH: &str = "gsp_00000000000000000000000000000000";

/// Every tool that writes, with arguments that would otherwise succeed.
fn write_calls() -> Vec<(&'static str, Value)> {
    vec![
        ("spec_apply", json!({"guild": "1000", "toml": SPEC, "plan_hash": HASH})),
        ("role_upsert", json!({"guild": "1000", "name": "New"})),
        ("role_delete", json!({"guild": "1000", "role": "@Member", "confirm": true})),
        ("channel_upsert", json!({"guild": "1000", "name": "new"})),
        ("channel_delete", json!({"guild": "1000", "channel": "#general", "confirm": true})),
        ("channels_reorder", json!({"guild": "1000", "order": ["#mod-log", "#general"]})),
        (
            "permissions_set",
            json!({"guild": "1000", "channel": "#general", "target": "@Member", "allow": ["view_channel"]}),
        ),
        ("member_roles", json!({"guild": "1000", "member": "sam", "add": ["Moderator"]})),
        ("message_send", json!({"guild": "1000", "channel": "#general", "content": "hi"})),
        ("member_timeout", json!({"guild": "1000", "member": "sam", "duration": 60})),
        ("member_kick", json!({"guild": "1000", "member": "sam", "confirm": true})),
        ("member_ban", json!({"guild": "1000", "member": "sam", "confirm": true})),
        ("ban_remove", json!({"guild": "1000", "user": "51"})),
    ]
}

/// Read tools, which keep working in read-only mode.
fn read_calls() -> Vec<(&'static str, Value)> {
    vec![
        ("guild_overview", json!({"guild": "1000"})),
        ("roles_list", json!({"guild": "1000"})),
        ("channels_tree", json!({"guild": "1000"})),
        ("spec_export", json!({"guild": "1000"})),
        ("spec_plan", json!({"guild": "1000", "toml": SPEC})),
        ("permissions_explain", json!({"guild": "1000", "channel": "#mod-log", "who": "@everyone"})),
        ("members_find", json!({"guild": "1000", "query": "s"})),
        ("messages_read", json!({"guild": "1000", "channel": "#general"})),
        ("bot_preflight", json!({"guild": "1000"})),
        ("refresh", json!({"guild": "1000"})),
    ]
}

fn app(policy: Policy) -> Arc<App<FakeDiscord>> {
    let policy = Policy { toolsets: BTreeSet::from([Toolset::Core, Toolset::Moderation]), ..policy };
    Arc::new(App::new(fixtures::lab_fake(), policy))
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

async fn call(c: &RunningService<RoleClient, Client>, name: &'static str, args: &Value) -> Value {
    let r =
        c.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())).await.unwrap();
    let mut sc = r.structured_content.unwrap_or(Value::Null);
    sc["is_error"] = Value::Bool(r.is_error == Some(true));
    sc
}

async fn cli(app: &Arc<App<FakeDiscord>>, args: &[&str]) -> (Exit, String) {
    let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = dispatch_with(&cli, app.clone(), &mut out, &mut err, &mut |_| Some(true)).await;
    (exit, String::from_utf8(err).unwrap())
}

fn spec_file() -> String {
    let path = std::env::temp_dir().join(format!("guildsmith-policy-{}.toml", std::process::id()));
    std::fs::write(&path, SPEC).unwrap();
    path.to_str().unwrap().to_owned()
}

#[tokio::test]
async fn servers_outside_the_allowlist_are_refused_everywhere() {
    let app = app(Policy { allowlist: Some(BTreeSet::from([Id(999)])), ..Policy::default() });
    let c = connect(app.clone()).await;
    for (tool, args) in write_calls().into_iter().chain(read_calls()) {
        let sc = call(&c, tool, &args).await;
        assert_eq!(sc["refusal"]["kind"], "not_allowed", "{tool}: {sc}");
    }
    let file = spec_file();
    for args in [
        vec!["export", "-g", "1000"],
        vec!["plan", "-g", "1000", &file],
        vec!["apply", "-g", "1000", &file, "--yes", "--plan-hash", HASH],
        vec!["apply", "-g", "1000", &file],
    ] {
        let (exit, err) = cli(&app, &args).await;
        assert_eq!(exit, Exit::Refused, "{args:?}: {err}");
        assert!(err.contains("not on the allowlist"), "{args:?}: {err}");
    }
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn read_only_refuses_every_write_before_any_request() {
    let app = app(Policy { read_only: true, ..Policy::default() });
    let c = connect(app.clone()).await;
    for (tool, args) in write_calls() {
        let sc = call(&c, tool, &args).await;
        assert_eq!(sc["refusal"]["kind"], "read_only", "{tool}: {sc}");
    }
    let file = spec_file();
    let (exit, err) = cli(&app, &["apply", "-g", "1000", &file, "--yes", "--plan-hash", HASH]).await;
    assert_eq!(exit, Exit::Refused, "{err}");
    assert_eq!(app.port.reads(), 0, "refused before any request");
    assert_eq!(app.port.writes(), []);

    for (tool, args) in read_calls() {
        let sc = call(&c, tool, &args).await;
        assert_eq!(sc["is_error"], false, "{tool} still works in read-only mode: {sc}");
    }
    let (exit, _) = cli(&app, &["plan", "-g", "1000", &file]).await;
    assert_eq!(exit, Exit::Ok);
    assert_eq!(app.port.writes(), []);
}

#[test]
fn an_empty_allowlist_warns_at_startup() {
    let parse = |args: &[&str]| Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).unwrap();
    let open = Policy::default();
    let warning = startup_warning(&parse(&["mcp"]).command, &open).expect("warns");
    assert!(warning.contains("no allowlist set; all servers the bot is in are reachable"), "{warning}");
    assert!(startup_warning(&parse(&["guilds"]).command, &open).is_some());
    assert!(startup_warning(&parse(&["schema"]).command, &open).is_none(), "offline commands stay quiet");
    let closed = Policy { allowlist: Some(BTreeSet::from([Id(1)])), ..Policy::default() };
    assert!(startup_warning(&parse(&["mcp"]).command, &closed).is_none());

    // And the real binary logs it to stderr.
    let out = common::run(&["mcp"], &[], "");
    assert!(out.stderr.contains("no allowlist set"), "{}", out.stderr);
    let out = common::run(&["mcp"], &[("GUILDSMITH_GUILDS", "1000")], "");
    assert!(!out.stderr.contains("no allowlist set"), "{}", out.stderr);
}
