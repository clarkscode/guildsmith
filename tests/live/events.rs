//! 002 quickstart §3 (US3): an event without a time zone is UTC and shown as "UTC"; invites
//! are made and revoked. Destructive: creates and deletes both.

use serde_json::json;

use crate::{destructive, live, mcp, ok};

#[test]
fn events_and_invites() {
    let Some(g) = live() else { return };
    let r = mcp(&[("events_list", json!({"guild": g})), ("invites_list", json!({"guild": g}))]);
    assert!(r.iter().all(ok), "{r:?}");
    if !destructive() {
        return;
    }
    let name = format!("guildsmith live {}", std::process::id());
    let r = mcp(&[
        (
            "event_create",
            json!({"guild": g, "name": name, "start": "2030-01-04T18:00:00", "end": "2030-01-04T20:00:00",
                "location": {"external": "The lab"}}),
        ),
        ("invite_create", json!({"guild": g, "channel": "#general", "max_age": 3600, "unique": true})),
    ]);
    assert!(ok(&r[0]), "{}", r[0]);
    assert!(r[0]["content"].to_string().contains("UTC"), "a time without a zone is UTC: {}", r[0]);
    let code = r[1]["structuredContent"]["code"].as_str().unwrap().to_owned();
    let r = mcp(&[
        ("event_delete", json!({"guild": g, "event": name, "confirm": true})),
        ("invite_revoke", json!({"guild": g, "code": code, "confirm": true})),
    ]);
    assert!(r.iter().all(ok), "{r:?}");
}
