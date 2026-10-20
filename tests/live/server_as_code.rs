//! 002 quickstart §4 (US9, SC-010/011): the test server is set up from the full-server example,
//! exported with its files, applied to a second, empty server, and both then plan to nothing.
//! Needs `GUILDSMITH_LIVE_EMPTY` (that server's id, also on the allowlist) and the destructive
//! flag.

use crate::{destructive, live, run};

fn plan_hash_in(g: &str, file: &str, env: &[(&str, &str)]) -> String {
    let out = run(&["plan", "-g", g, file, "--json"], env, "");
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    v["plan_hash"].as_str().unwrap().to_owned()
}

fn apply(g: &str, file: &str, env: &[(&str, &str)]) -> crate::Out {
    let hash = plan_hash_in(g, file, env);
    let out = run(&["apply", "-g", g, file, "--yes", "--plan-hash", &hash], env, "");
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    out
}

fn converged(g: &str, file: &str, env: &[(&str, &str)]) {
    let p = run(&["plan", "-g", g, file, "--detailed-exitcode"], env, "");
    assert_eq!(p.code, Some(0), "{g} converges:\n{}", p.stdout);
}

#[test]
fn export_copy_and_converge() {
    let Some(lab) = live() else { return };
    let Ok(empty) = std::env::var("GUILDSMITH_LIVE_EMPTY") else {
        eprintln!("skipped: set GUILDSMITH_LIVE_EMPTY to a second, empty test server");
        return;
    };
    if !destructive() {
        return;
    }
    let both = format!("{lab},{empty}");
    let dir = std::env::temp_dir().join(format!("guildsmith-live-sac-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let roots = dir.to_str().unwrap().to_owned();
    let env = [("GUILDSMITH_GUILDS", both.as_str()), ("GUILDSMITH_FILE_ROOTS", roots.as_str())];

    let example = format!("{}/../../examples/full-server/guild.toml", env!("CARGO_MANIFEST_DIR"));
    apply(&lab, &example, &env);
    converged(&lab, &example, &env);

    let lab_toml = dir.join("lab.toml");
    let file = lab_toml.to_str().unwrap();
    let e = run(&["export", "-g", &lab, "-o", file], &env, "");
    assert_eq!(e.code, Some(0), "{}", e.stderr);
    let text = std::fs::read_to_string(&lab_toml).unwrap();
    assert!(!text.contains("/api/webhooks/") && !text.to_lowercase().contains("token"), "SC-011");
    assert!(dir.join("assets").is_dir(), "files are saved next to the export");
    converged(&lab, file, &env);

    let a = apply(&empty, file, &env);
    assert!(a.stderr.contains("secret: copy now"), "the copy's webhook URL is shown once:\n{}", a.stderr);
    converged(&empty, file, &env);
    converged(&lab, file, &env);
}
