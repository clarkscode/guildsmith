//! 002 quickstart §1 and §3 (US8, US2): a group enabled at runtime through the dispatcher, then
//! messages, threads and polls in the test server's #general. Destructive: posts and deletes.

use serde_json::json;

use crate::{destructive, live, mcp, ok, run};

#[test]
fn the_default_list_has_23_tools_and_groups_unlock_at_runtime() {
    let Some(g) = live() else { return };
    let session = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25",
            "capabilities": {}, "clientInfo": {"name": "live", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "toolsets",
            "arguments": {"action": "enable", "toolset": "events"}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "tool_call",
            "arguments": {"tool": "events_list", "arguments": {"guild": g}}}}),
    ]
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let out = run(&["mcp"], &[("GUILDSMITH_TOOLSETS", "")], &session);
    let lines: Vec<serde_json::Value> = out.stdout.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let list = lines.iter().find(|v| v["id"] == 2).unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 23, "SC-002");
    let call = lines.iter().find(|v| v["id"] == 4).unwrap();
    assert!(call["result"]["isError"] != true, "{call}");
    assert!(out.stderr.contains("events"), "the enablement is logged: {}", out.stderr);
}

#[test]
fn messages_threads_and_polls() {
    let Some(g) = live() else { return };
    if !destructive() {
        return;
    }
    let tag = format!("guildsmith live {}", std::process::id());
    let r = mcp(&[
        ("message_compose", json!({"guild": g, "channel": "#general", "content": tag})),
        ("thread_create", json!({"guild": g, "channel": "#general", "name": tag})),
        (
            "poll_create",
            json!({"guild": g, "channel": "#general", "question": tag, "answers": ["yes", "no"], "duration": 1}),
        ),
        ("messages_read", json!({"guild": g, "channel": "#general", "limit": 5})),
    ]);
    for x in &r {
        assert!(ok(x), "{x}");
    }
    let text = r[3]["content"].to_string();
    assert!(text.contains("discord-message"), "read messages are wrapped as untrusted: {text}");
    let id = r[0]["structuredContent"]["id"].as_str().unwrap_or_default().to_owned();
    let cleanup =
        mcp(&[("message_delete", json!({"guild": g, "channel": "#general", "message": id, "confirm": true}))]);
    assert!(ok(&cleanup[0]), "{}", cleanup[0]);
}
