//! `guildsmith export -g <guild> [-o <path>] [--force]`.

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use guildsmith_app::App;
use guildsmith_app::date;
use guildsmith_app::ports::{DiscordPort, SystemClock};
use guildsmith_app::services::export;

use super::Exit;
use crate::render;

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Write the spec here instead of stdout. Refuses to overwrite without --force.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    /// Overwrite the output file if it exists.
    #[arg(long)]
    pub force: bool,
}

pub async fn run<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    args: &ExportArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    // With -o, files are saved under assets/ next to the spec file; on stdout they stay urls.
    if let Some(path) = &args.output
        && path.exists()
        && !args.force
    {
        return render::service_error(
            err,
            &guildsmith_app::error::ServiceError::Invalid(format!(
                "{} already exists; refusing to overwrite it (use --force to replace it)",
                path.display()
            )),
        );
    }
    let dir = args.output.as_deref().map(super::plan::spec_dir);
    let result = match export::export_to(app, guild, &date::today(&SystemClock), dir.as_deref()).await {
        Ok(r) => r,
        Err(e) => return render::service_error(err, &e),
    };
    for w in &result.warnings {
        let _ = writeln!(err, "warning: {w}");
    }
    match &args.output {
        Some(path) => {
            if let Err(e) = export::write_file(path, &result.toml, args.force) {
                return render::service_error(err, &e);
            }
            let _ = writeln!(err, "wrote {}", path.display());
        }
        None => {
            let _ = write!(out, "{}", result.toml);
        }
    }
    Exit::Ok
}
