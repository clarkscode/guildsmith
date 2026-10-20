//! 002 quickstart §3 (US7, SC-009): settings read back, and a public template (when
//! `GUILDSMITH_LIVE_TEMPLATE` names one) imports to a spec that validates.

use serde_json::json;

use crate::{live, mcp, ok};

#[test]
fn settings_and_template_import() {
    let Some(g) = live() else { return };
    let r = mcp(&[("settings_get", json!({"guild": g}))]);
    assert!(ok(&r[0]), "{}", r[0]);
    let Ok(code) = std::env::var("GUILDSMITH_LIVE_TEMPLATE") else {
        eprintln!("skipped template import: set GUILDSMITH_LIVE_TEMPLATE to a public template code");
        return;
    };
    let r = mcp(&[("template_import", json!({"code_or_url": code}))]);
    assert!(ok(&r[0]), "{}", r[0]);
    let toml = r[0]["structuredContent"]["toml"].as_str().unwrap().to_owned();
    let v = mcp(&[("spec_validate", json!({"toml": toml}))]);
    assert_eq!(v[0]["structuredContent"]["valid"], true, "SC-009: {}", v[0]);
}
