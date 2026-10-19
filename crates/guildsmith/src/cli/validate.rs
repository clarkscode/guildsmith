//! `guildsmith validate <file>`: offline checks, problems as `file:line:col`.

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use guildsmith_app::services::validate;

use super::Exit;
use crate::render;

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// The spec file to check.
    pub file: PathBuf,
}

pub fn run(args: &ValidateArgs, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let text = match std::fs::read_to_string(&args.file) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "error: can't read {}: {e}", args.file.display());
            return Exit::Error;
        }
    };
    let result = validate::validate(&text);
    if json {
        render::json(out, &result);
    } else {
        render::problems(out, &args.file.display().to_string(), &result.problems);
        if result.valid {
            let _ = writeln!(out, "{}: valid", args.file.display());
        }
    }
    if result.valid { Exit::Ok } else { Exit::Error }
}
