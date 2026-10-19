//! Runs the real `guildsmith` binary for end-to-end tests.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Stdio};

/// The canary token seeded by secret-leak tests (quickstart §9).
pub const CANARY: &str = "gs-canary-7f3a";

/// What a finished process wrote.
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

/// A clean environment: no inherited Discord or guildsmith settings, no real config file.
pub fn command(args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_guildsmith"));
    cmd.args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::temp_dir().join("guildsmith-test-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

/// Runs a command, feeding `stdin` and closing it.
pub fn run(args: &[&str], env: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = command(args, env).spawn().expect("spawn guildsmith");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait for guildsmith");
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code(),
    }
}

/// JSON-RPC lines for an MCP session: handshake, `tools/list`, then `calls` as `tools/call`.
pub fn mcp_session(calls: &[(&str, serde_json::Value)]) -> String {
    let mut lines = vec![
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}
        }}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ];
    for (i, (name, args)) in calls.iter().enumerate() {
        lines.push(serde_json::json!({"jsonrpc": "2.0", "id": 10 + i, "method": "tools/call",
            "params": {"name": name, "arguments": args}}));
    }
    lines.iter().fold(String::new(), |mut out, l| {
        let _ = writeln!(out, "{l}");
        out
    })
}
