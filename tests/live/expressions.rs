//! 002 quickstart §3 (US6, T074): an emoji uploads, and the CDN serves the same bytes (the
//! research E7 check). Destructive: creates and deletes an emoji.

use serde_json::json;

use crate::{destructive, live, mcp, ok};

/// A real 16×16 PNG.
fn png() -> Vec<u8> {
    let path = format!("{}/../../examples/full-server/assets/emoji/guildsmith.png", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
}

#[test]
fn emoji_round_trip_keeps_the_bytes() {
    let Some(g) = live() else { return };
    assert!(ok(&mcp(&[("expressions_list", json!({"guild": g}))])[0]));
    if !destructive() {
        return;
    }
    let bytes = png();
    let data = format!("data:image/png;base64,{}", base64_encode(&bytes));
    let name = format!("gs_live_{}", std::process::id());
    let r = mcp(&[("expression_create", json!({"guild": g, "kind": "emoji", "name": name, "asset": {"data": data}}))]);
    assert!(ok(&r[0]), "{}", r[0]);
    // A plan with this emoji converges only if the CDN keeps the bytes (T074).
    let spec =
        crate::temp_file("emoji.toml", &format!("[[emoji]]\nname = \"{name}\"\nimage = {{ data = \"{data}\" }}\n"));
    let p = crate::run(&["plan", "-g", &g, &spec, "--detailed-exitcode"], &[], "");
    assert_eq!(p.code, Some(0), "the CDN returned different bytes (record this in research E7):\n{}", p.stdout);
    let r = mcp(&[("expression_delete", json!({"guild": g, "kind": "emoji", "item": name, "confirm": true}))]);
    assert!(ok(&r[0]), "{}", r[0]);
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n =
            (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
