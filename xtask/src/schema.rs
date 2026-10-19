//! `cargo xtask schema [--check]`: writes `schema/guild.schema.json` from the core types, or
//! with `--check`, fails if the committed file is out of date (CI drift check).

use std::path::Path;

use anyhow::{Result, bail};

pub fn run(args: &[String]) -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../schema/guild.schema.json");
    let text = guildsmith_core::spec::json_schema_text();
    if args.iter().any(|a| a == "--check") {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != text {
            bail!("schema/guild.schema.json is out of date; run `cargo xtask schema` and commit it");
        }
        println!("schema/guild.schema.json is up to date");
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("schema dir"))?;
    std::fs::write(&path, text)?;
    println!("wrote schema/guild.schema.json");
    Ok(())
}
