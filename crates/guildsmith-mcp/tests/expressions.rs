//! T073: the `expressions` group over MCP: the 4 tools with their annotations and outputs that
//! match their schemas.

mod common;

use common::{Ann, assert_valid, call, check_group, connect};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::toolsets::Toolset;
use guildsmith_core::asset::{FileType, data_uri};
use serde_json::json;

const TOOLS: &[(&str, Ann)] = &[
    ("expressions_list", Ann::Ro),
    ("expression_create", Ann::W),
    ("expression_update", Ann::Wi),
    ("expression_delete", Ann::D),
];

#[tokio::test]
async fn expressions_over_mcp() {
    let app = common::app(fixtures::lab_fake(), &[Toolset::Expressions], Policy::default());
    let c = connect(app.clone()).await;
    let tools = check_group(&c, Toolset::Expressions, TOOLS).await;
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    png.extend(b"IDAT");
    let g = "1000";
    let created = call(
        &c,
        "expression_create",
        json!({"guild": g, "kind": "emoji", "name": "logo",
        "asset": {"data": data_uri(FileType::Png, &png)}}),
    )
    .await;
    assert_valid(&tools, "expression_create", &created);
    for (name, args) in [
        ("expressions_list", json!({"guild": g})),
        ("expression_update", json!({"guild": g, "kind": "emoji", "item": "logo", "name": "logo2"})),
        ("expression_delete", json!({"guild": g, "kind": "emoji", "item": "logo2"})),
    ] {
        let r = call(&c, name, args).await;
        assert_valid(&tools, name, &r);
    }
    let big = call(
        &c,
        "expression_create",
        json!({"guild": g, "kind": "sound", "name": "xx", "asset": {"data": data_uri(FileType::Png, &png)}}),
    )
    .await;
    assert_eq!(big.is_error, Some(true));
    assert!(big.structured_content.unwrap()["error"].as_str().unwrap().contains("MP3/Ogg"));
}
