//! Repository automation: `cargo xtask <command>`.

mod budgets;
mod schema;

use anyhow::{Result, bail};

const USAGE: &str = "usage: cargo xtask <schema|budgets|sync-plugins|npm-pack>";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("budgets") => budgets::run(&args[1..]),
        Some("schema") => schema::run(&args[1..]),
        // Filled in by later tasks; each stub exits 0 so CI can call it from M0 on.
        Some("sync-plugins" | "npm-pack") => Ok(()),
        Some(other) => bail!("unknown command `{other}`\n{USAGE}"),
        None => bail!("{USAGE}"),
    }
}
