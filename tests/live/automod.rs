//! 002 quickstart §3 (US4): an automod rule is created, listed and deleted. Destructive.

use serde_json::json;

use crate::{destructive, live, mcp, ok};

#[test]
fn rule_round_trip() {
    let Some(g) = live() else { return };
    assert!(ok(&mcp(&[("automod_rules_list", json!({"guild": g}))])[0]));
    if !destructive() {
        return;
    }
    let name = format!("guildsmith live {}", std::process::id());
    let r = mcp(&[
        (
            "automod_rule_upsert",
            json!({"guild": g, "name": name, "trigger": {"keyword": {"keywords": ["guildsmith-live-test"]}},
                "actions": [{"block": {}}], "confirm": true}),
        ),
        ("automod_rule_delete", json!({"guild": g, "rule": name, "confirm": true})),
    ]);
    assert!(r.iter().all(ok), "{r:?}");
}
