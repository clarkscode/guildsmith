//! Repository automation: `cargo xtask <command>`.

mod budgets;
mod npm;
mod plugins;
mod schema;
mod sync;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const USAGE: &str = "usage: cargo xtask <command>

commands:
  schema [--check]                         write (or check) schema/guild.schema.json
  budgets [--bin <path>]                   startup, size and token budgets
  sync-plugins                             copy plugins/skills/ into each plugin
  check-plugins                            check plugin manifests and skill copies
  npm-pack <artifacts-dir> [--out <dir>] [--allow-missing]
                                           stage the npm packages from dist artifacts";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("budgets") => budgets::run(&args[1..]),
        Some("schema") => schema::run(&args[1..]),
        Some("sync-plugins") => sync::run(&args[1..]),
        Some("check-plugins") => plugins::run(&args[1..]),
        Some("npm-pack") => npm::run(&args[1..]),
        Some(other) => bail!("unknown command `{other}`\n{USAGE}"),
        None => bail!("{USAGE}"),
    }
}

/// The repository root.
pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the repo root").to_path_buf()
}

/// `[workspace.package] version` from the root `Cargo.toml`: the version every package ships.
pub(crate) fn workspace_version() -> Result<String> {
    let path = root().join("Cargo.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let doc: toml::Table = text.parse().with_context(|| format!("parse {}", path.display()))?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .context("Cargo.toml has no [workspace.package] version")
}

/// The value of `--name <value>` in `args`.
pub(crate) fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].as_str())
}
