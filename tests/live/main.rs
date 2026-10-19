//! Opt-in live tests: the quickstart (specs/001-guildsmith-v1/quickstart.md §1–§7) against a
//! real Discord test server. Skipped unless `GUILDSMITH_LIVE_TEST=1`.
//!
//! Needs `DISCORD_TOKEN` and `GUILDSMITH_GUILDS` (one dedicated test server). Tests that change
//! the server also need `GUILDSMITH_LIVE_DESTRUCTIVE=1`: they reset it to each example file.
//! Run with `cargo test -p guildsmith --test live -- --test-threads=1`.

mod agent;
mod apply;
mod export;
mod hidden_channel;
mod policy;
mod preflight;

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
