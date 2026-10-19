//! Quickstart §2–§3: each example applies and converges, and a server changed between plan
//! and apply is refused (US2, SC-002). Destructive: resets the test server to each example.

use crate::{destructive, live, plan_hash, run, temp_file};

/// The example with pruning on, so the test server ends up exactly like it.
fn exact(name: &str) -> String {
    let path = format!("{}/../../examples/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let text = text.replace("[prune]\nroles = false\ncategories = false\nchannels = false\n", "");
    format!("{text}\n[prune]\nroles = true\ncategories = true\nchannels = true\n")
}

#[test]
fn examples_apply_and_converge() {
    let Some(g) = live() else { return };
    if !destructive() {
        return;
    }
    for name in ["community", "dev-team", "gaming"] {
        let file = temp_file(&format!("{name}.toml"), &exact(name));
        let hash = plan_hash(&g, &file);
        let a = run(&["apply", "-g", &g, &file, "--yes", "--plan-hash", &hash], &[], "");
        assert_eq!(a.code, Some(0), "{name}:\n{}", a.stderr);
        assert!(a.stderr.contains("Applied"), "{}", a.stderr);
        let p = run(&["plan", "-g", &g, &file, "--detailed-exitcode"], &[], "");
        assert_eq!(p.code, Some(0), "{name} converges:\n{}", p.stdout);
    }
}

#[test]
fn a_server_changed_after_plan_is_refused() {
    let Some(g) = live() else { return };
    if !destructive() {
        return;
    }
    let file = temp_file("changed.toml", &exact("community"));
    let hash = plan_hash(&g, &file);
    // Change the server outside the plan: rename a channel with the live tools.
    let session = mcp_rename(&g);
    let out = run(&["mcp"], &[], &session);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let a = run(&["apply", "-g", &g, &file, "--yes", "--plan-hash", &hash], &[], "");
    assert_eq!(a.code, Some(3), "{}", a.stderr);
    assert!(a.stderr.contains("server changed since plan"), "{}", a.stderr);
}

fn mcp_rename(g: &str) -> String {
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "live", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "channel_upsert",
            "arguments": {"guild": g, "channel": "#general", "topic": format!("changed {}", std::process::id())}}}),
    ];
    lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n") + "\n"
}
