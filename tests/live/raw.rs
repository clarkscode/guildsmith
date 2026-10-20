//! 002 quickstart §5 (US10, SC-012): raw GETs work and come back wrapped, writes preview,
//! read-only and the allowlist refuse, and token routes are refused.

use crate::{live, run};

#[test]
fn raw_requests_follow_the_rules() {
    let Some(g) = live() else { return };
    let get = run(&["raw", "GET", "guilds/{guild}/audit-logs", "-g", &g, "--query", "limit=5"], &[], "");
    assert_eq!(get.code, Some(0), "{}", get.stderr);
    assert!(get.stdout.contains("discord-data") || get.stderr.contains("discord-data"), "wrapped as untrusted");

    let preview = run(&["raw", "PATCH", "guilds/{guild}", "-g", &g, "--body", r#"{"description":"x"}"#], &[], "");
    assert_eq!(preview.code, Some(3), "{}", preview.stderr);
    assert!(preview.stderr.contains("Preview only"), "{}", preview.stderr);

    let ro = run(
        &["raw", "PATCH", "guilds/{guild}", "-g", &g, "--body", "{}", "--yes"],
        &[("GUILDSMITH_READ_ONLY", "1")],
        "",
    );
    assert_eq!(ro.code, Some(3), "{}", ro.stderr);

    let outside = run(&["raw", "GET", "guilds/1"], &[], "");
    assert_eq!(outside.code, Some(3), "{}", outside.stderr);

    let token = run(&["raw", "POST", "webhooks/1/abc", "--yes"], &[], "");
    assert_eq!(token.code, Some(3), "{}", token.stderr);
}
