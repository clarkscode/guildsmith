//! Quickstart §5: preflight lists every problem before any change (US3/US5, SC-009).
//!
//! Needs a manual setup first: remove Administrator and Manage Roles from the bot's role and
//! move a role above it, then set `GUILDSMITH_LIVE_WEAK_BOT=1` and `GUILDSMITH_LIVE_HIGH_ROLE`
//! to that role's name.

use crate::{live, plan_hash, run, temp_file};

#[test]
fn preflight_reports_every_problem_together() {
    let Some(g) = live() else { return };
    let Ok(role) = std::env::var("GUILDSMITH_LIVE_HIGH_ROLE") else {
        eprintln!("skipped: needs GUILDSMITH_LIVE_WEAK_BOT=1 and GUILDSMITH_LIVE_HIGH_ROLE (see the module docs)");
        return;
    };
    assert_eq!(std::env::var("GUILDSMITH_LIVE_WEAK_BOT").as_deref(), Ok("1"));
    let doctor = run(&["doctor", "-g", &g], &[], "");
    assert_eq!(doctor.code, Some(3), "{}", doctor.stdout);
    assert!(doctor.stdout.contains("manage_roles") && doctor.stdout.contains(&role), "{}", doctor.stdout);

    let file = temp_file("preflight.toml", &format!("[[roles]]\nname = \"{role}\"\nhoist = true\n"));
    let hash = plan_hash(&g, &file);
    let a = run(&["apply", "-g", &g, &file, "--yes", "--plan-hash", &hash], &[], "");
    assert_eq!(a.code, Some(3), "{}", a.stderr);
    for needle in ["not an administrator", "manage_roles", &role] {
        assert!(a.stderr.contains(needle), "missing `{needle}` in one report:\n{}", a.stderr);
    }
}
