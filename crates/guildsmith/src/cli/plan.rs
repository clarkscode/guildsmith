//! `guildsmith plan -g <guild> <file> [--json] [--detailed-exitcode]` (FR-007, FR-012).

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::plan;

use super::Exit;
use crate::render;

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// The spec file to plan.
    pub file: PathBuf,
    /// Exit 0 when there are no changes and 2 when there are.
    #[arg(long)]
    pub detailed_exitcode: bool,
}

/// Reads a spec file, printing the error and returning `None` when it can't be read.
pub(crate) fn read_spec(file: &PathBuf, err: &mut dyn Write) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(t) => Some(t),
        Err(e) => {
            let _ = writeln!(err, "error: can't read {}: {e}", file.display());
            None
        }
    }
}

pub async fn run<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    args: &PlanArgs,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let Some(text) = read_spec(&args.file, err) else { return Exit::Error };
    let result = match plan::plan(app, guild, &text).await {
        Ok(p) => p,
        Err(e) => return render::service_error(err, &e),
    };
    if json {
        render::json(out, &result);
    } else {
        render::plan(out, &result);
    }
    if args.detailed_exitcode && result.has_changes() { Exit::Changes } else { Exit::Ok }
}
