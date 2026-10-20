//! Opt-in live tests: the quickstarts (specs/001-guildsmith-v1/quickstart.md §1–§7 and
//! specs/002-extended-toolsets/quickstart.md §1–§5) against a real Discord test server.
//! Skipped unless `GUILDSMITH_LIVE_TEST=1`.
//!
//! Needs `DISCORD_TOKEN` and `GUILDSMITH_GUILDS` (one dedicated test server). Tests that change
//! the server also need `GUILDSMITH_LIVE_DESTRUCTIVE=1`: they reset it to each example file.
//! The 002 tests also read `GUILDSMITH_LIVE_EMPTY` (a second, empty server for the
//! server-as-code copy) and `GUILDSMITH_LIVE_ALTS` (test accounts' ids for moderation).
//! Run with `cargo test -p guildsmith --test live -- --test-threads=1`.

mod agent;
mod apply;
mod automod;
mod community;
mod events;
mod export;
mod expressions;
mod hidden_channel;
mod moderation;
mod policy;
mod preflight;
mod raw;
mod server_as_code;
mod settings;
mod webhooks;

use std::io::Write;
use std::process::{Command, Stdio};

/// What a finished `guildsmith` run wrote.
pub(crate) struct Out {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

/// The test server id, or `None` when live tests are off.
pub(crate) fn live() -> Option<String> {
    if std::env::var("GUILDSMITH_LIVE_TEST").as_deref() != Ok("1") {
        eprintln!("skipped: set GUILDSMITH_LIVE_TEST=1 to run live tests");
        return None;
    }
    let guild = std::env::var("GUILDSMITH_GUILDS").expect("GUILDSMITH_GUILDS: the test server id");
    assert!(!guild.contains(','), "GUILDSMITH_GUILDS must name exactly one test server");
    assert!(std::env::var("DISCORD_TOKEN").is_ok(), "DISCORD_TOKEN is required for live tests");
    Some(guild)
}

/// Whether tests that change the server may run.
pub(crate) fn destructive() -> bool {
    let on = std::env::var("GUILDSMITH_LIVE_DESTRUCTIVE").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: set GUILDSMITH_LIVE_DESTRUCTIVE=1 to let live tests change the test server");
    }
    on
}

/// Runs the real binary with the caller's environment plus `env`, feeding `stdin`.
pub(crate) fn run(args: &[&str], env: &[(&str, &str)], stdin: &str) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_guildsmith"));
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn guildsmith");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code(),
    }
}

/// A file in the temp directory with `text`.
pub(crate) fn temp_file(name: &str, text: &str) -> String {
    let path = std::env::temp_dir().join(format!("guildsmith-live-{}-{name}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

/// The plan hash from `guildsmith plan --json`.
pub(crate) fn plan_hash(guild: &str, file: &str) -> String {
    let out = run(&["plan", "-g", guild, file, "--json"], &[], "");
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    v["plan_hash"].as_str().unwrap().to_owned()
}

/// Runs one `guildsmith mcp` session (every group registered) calling each tool in order, and
/// returns each call's result (`structuredContent`, text and `isError`), in order.
pub(crate) fn mcp(calls: &[(&str, serde_json::Value)]) -> Vec<serde_json::Value> {
    let mut lines = vec![
        serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "live", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for (i, (tool, args)) in calls.iter().enumerate() {
        lines.push(serde_json::json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call",
            "params": {"name": tool, "arguments": args}}));
    }
    let session = lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n") + "\n";
    let out = run(&["mcp"], &[("GUILDSMITH_TOOLSETS", "all")], &session);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let mut results: Vec<(u64, serde_json::Value)> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| Some((v["id"].as_u64().filter(|i| *i > 0)?, v["result"].clone())))
        .collect();
    results.sort_by_key(|(i, _)| *i);
    assert_eq!(results.len(), calls.len(), "a result per call:\n{}", out.stdout);
    results.into_iter().map(|(_, r)| r).collect()
}

/// Whether a tool result succeeded.
pub(crate) fn ok(r: &serde_json::Value) -> bool {
    r["isError"] != true
}

/// The ids of test accounts for moderation, if given.
pub(crate) fn alts() -> Vec<String> {
    std::env::var("GUILDSMITH_LIVE_ALTS")
        .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}
