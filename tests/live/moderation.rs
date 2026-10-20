//! 002 quickstart §2 (US1): bans preview without confirm, ban and unban test accounts with it,
//! and prune counts only. Banning needs `GUILDSMITH_LIVE_ALTS` and the destructive flag.

use serde_json::json;

use crate::{alts, destructive, live, mcp, ok};

#[test]
fn previews_change_nothing() {
    let Some(g) = live() else { return };
    let mut members = alts();
    if members.is_empty() {
        members.push("1".to_owned());
    }
    let r = mcp(&[
        ("member_ban", json!({"guild": g, "members": members})),
        ("members_prune", json!({"guild": g, "days": 30})),
        ("bans_list", json!({"guild": g})),
    ]);
    assert!(ok(&r[0]) && r[0]["structuredContent"]["preview"] == true, "{}", r[0]);
    assert!(ok(&r[1]) && r[1]["structuredContent"]["preview"] == true, "prune without confirm only counts: {}", r[1]);
    assert!(ok(&r[2]), "{}", r[2]);
}

#[test]
fn ban_and_unban_test_accounts() {
    let Some(g) = live() else { return };
    let members = alts();
    if members.is_empty() || !destructive() {
        eprintln!("skipped: needs GUILDSMITH_LIVE_ALTS and GUILDSMITH_LIVE_DESTRUCTIVE=1");
        return;
    }
    let started = std::time::Instant::now();
    let r = mcp(&[("member_ban", json!({"guild": g, "members": members, "confirm": true, "reason": "live test"}))]);
    let o = &r[0]["structuredContent"];
    let total =
        ["succeeded", "failed", "not_attempted"].iter().map(|k| o[k].as_array().map_or(0, Vec::len)).sum::<usize>();
    assert_eq!(total, members.len(), "the outcome adds up (SC-005): {o}");
    assert!(started.elapsed().as_secs() < 120, "SC-008");
    let unban: Vec<_> = members.iter().map(|m| ("ban_remove", json!({"guild": g, "user": m}))).collect();
    assert!(mcp(&unban).iter().all(ok));
}
