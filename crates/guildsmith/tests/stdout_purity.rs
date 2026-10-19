//! T019: in MCP mode, stdout carries only JSON-RPC; every log line goes to stderr
//! (constitution IV).

mod common;

#[test]
fn every_stdout_line_is_json_rpc_even_at_trace() {
    let session = common::mcp_session(&[("no_such_tool", serde_json::json!({}))]);
    let out = common::run(&["mcp", "--log", "trace"], &[("DISCORD_TOKEN", common::CANARY)], &session);

    let lines: Vec<&str> = out.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines.len() >= 3, "expected responses for initialize, tools/list and the call:\n{}", out.stdout);
    for line in &lines {
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        assert_eq!(v["jsonrpc"], "2.0", "not a JSON-RPC message: {line}");
    }
    // The failing call still produced a JSON-RPC reply, not stray text.
    assert!(lines.iter().any(|l| l.contains("\"id\":10")), "{}", out.stdout);
    // Trace logging happened, and all of it went to stderr.
    assert!(out.stderr.contains("TRACE") || out.stderr.contains("DEBUG"), "expected logs on stderr:\n{}", out.stderr);
    assert!(!out.stdout.contains("TRACE") && !out.stdout.contains(" INFO "), "logs leaked into stdout");
    assert_eq!(out.code, Some(0), "server exits cleanly when stdin closes; stderr:\n{}", out.stderr);
}
