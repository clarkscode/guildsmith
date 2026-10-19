//! `guildsmith schema [-o <path>]`: the JSON Schema for spec files.

use std::io::Write;
use std::path::PathBuf;

use clap::Args;

use super::Exit;

#[derive(Debug, Args)]
pub struct SchemaArgs {
    /// Write the schema here instead of stdout.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}

pub fn run(args: &SchemaArgs, out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let text = guildsmith_core::spec::json_schema_text();
    let Some(path) = &args.output else {
        let _ = write!(out, "{text}");
        return Exit::Ok;
    };
    if let Err(e) = std::fs::write(path, text) {
        let _ = writeln!(err, "error: can't write {}: {e}", path.display());
        return Exit::Error;
    }
    Exit::Ok
}
