//! 002 quickstart §3 (US5, SC-006): a new webhook's URL appears once, never in lists or logs.
//! Destructive: creates and deletes a webhook.

use serde_json::json;

use crate::{destructive, live, mcp, ok, run};

#[test]
fn the_url_appears_once() {
    let Some(g) = live() else { return };
    if !destructive() {
        return;
    }
    let name = format!("live {}", std::process::id());
    let r = mcp(&[("webhook_create", json!({"guild": g, "channel": "#general", "name": name}))]);
    assert!(ok(&r[0]), "{}", r[0]);
    let url = r[0]["structuredContent"]["secrets"]["url"].as_str().unwrap().to_owned();
    let token = url.rsplit('/').next().unwrap().to_owned();
    let r = mcp(&[
        ("webhooks_list", json!({"guild": g})),
        ("webhook_send", json!({"guild": g, "webhook": name, "content": "guildsmith live test"})),
        ("webhook_delete", json!({"guild": g, "webhook": name, "confirm": true})),
    ]);
    assert!(r.iter().all(ok), "{r:?}");
    assert!(!format!("{r:?}").contains(&token), "lists and sends never show the token");
    let out = run(&["--log", "trace", "webhooks", "webhooks-list", "-g", &g], &[("GUILDSMITH_TOOLSETS", "all")], "");
    assert!(!out.stdout.contains(&token) && !out.stderr.contains(&token), "nor the logs");
}
