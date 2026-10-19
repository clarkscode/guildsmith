//! Quickstart §4: policy refusals (US3, SC-007).

use crate::{live, run, temp_file};

#[test]
fn policy_refusals_exit_3() {
    let Some(g) = live() else { return };
    let file = temp_file("policy.toml", "[[roles]]\nname = \"Live Test Role\"\n");
    let hash = "gsp_00000000000000000000000000000000";
    let ro = run(&["apply", "-g", &g, &file, "--plan-hash", hash, "--yes"], &[("GUILDSMITH_READ_ONLY", "1")], "");
    assert_eq!(ro.code, Some(3), "{}", ro.stderr);
    assert!(ro.stderr.contains("read-only"), "{}", ro.stderr);
    let yes = run(&["apply", "-g", &g, &file, "--yes"], &[], "");
    assert_eq!(yes.code, Some(3));
    assert!(yes.stderr.contains("--yes needs --plan-hash"), "{}", yes.stderr);
    let other = run(&["export", "-g", "1"], &[], "");
    assert_eq!(other.code, Some(3), "a server outside the allowlist: {}", other.stderr);
}
