//! `guildsmith apply -g <guild> <file> [--yes] [--plan-hash <hash>] [--json]` (contracts/cli.md).
//!
//! - Interactive (no `--yes`): show the plan and ask on a TTY. The hash of the plan that was
//!   shown is what gets applied, so a server change before the user confirms is refused.
//! - Non-interactive (`--yes`): needs `--plan-hash` from a reviewed plan (constitution I).
//!
//! Either way the apply service takes a fresh snapshot and refuses unless the hash matches.

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use guildsmith_app::App;
use guildsmith_app::policy::{Refusal, RefusalKind};
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::apply::{self, Progress};
use guildsmith_app::services::plan;

use super::Exit;
use super::plan::read_spec;
use crate::render;

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// The spec file to apply.
    pub file: PathBuf,
    /// Don't ask for confirmation. Needs --plan-hash from a reviewed plan.
    #[arg(long)]
    pub yes: bool,
    /// The hash of the plan that was reviewed (from `guildsmith plan`).
    #[arg(long, value_name = "HASH")]
    pub plan_hash: Option<String>,
}

/// Asks the user to confirm. `None` means there is no terminal to ask on.
pub type Confirm<'a> = dyn FnMut(&str) -> Option<bool> + Send + 'a;

fn refused(err: &mut dyn Write, kind: RefusalKind, reason: &str, hint: &str) -> Exit {
    render::service_error(err, &Refusal::new(kind, reason, Some(hint)).into())
}

pub async fn run<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    args: &ApplyArgs,
    json: bool,
    io: (&mut dyn Write, &mut dyn Write),
    confirm: &mut Confirm<'_>,
) -> Exit {
    let (out, err) = io;
    if args.yes && args.plan_hash.is_none() {
        return refused(
            err,
            RefusalKind::NeedsConfirmation,
            "--yes needs --plan-hash from a reviewed plan",
            "run `guildsmith plan` first, review it, then pass its plan hash with --plan-hash",
        );
    }
    if let Err(r) = app.policy.ensure_writable() {
        return render::service_error(err, &r.into());
    }
    let Some(text) = read_spec(&args.file, err) else { return Exit::Error };

    let hash = if args.yes {
        args.plan_hash.clone().unwrap_or_default()
    } else {
        // Show the plan, then apply exactly that plan's hash.
        let shown = match plan::plan(app, guild, &text).await {
            Ok(p) => p,
            Err(e) => return render::service_error(err, &e),
        };
        render::plan(err, &shown);
        if !shown.has_changes() {
            return Exit::Ok;
        }
        if args.plan_hash.as_deref().is_some_and(|h| h != shown.plan_hash) {
            return refused(
                err,
                RefusalKind::PlanMismatch,
                "server changed since plan, or --plan-hash belongs to a different plan; nothing was changed",
                "run plan again, review it, and apply with the new plan hash",
            );
        }
        match confirm(&format!("Apply these {} change(s)? [y/N] ", shown.ops.len())) {
            Some(true) => shown.plan_hash,
            Some(false) => {
                let _ = writeln!(err, "Cancelled; nothing was changed.");
                return Exit::Refused;
            }
            None => {
                return refused(
                    err,
                    RefusalKind::NeedsConfirmation,
                    "not a terminal, so there is no one to confirm; nothing was changed",
                    "review with `guildsmith plan`, then run apply with --yes --plan-hash <hash>",
                );
            }
        }
    };

    let mut progress = |p: &Progress| {
        eprintln!("[{}/{}] {}", p.index, p.total, p.summary);
    };
    match apply::apply(app, guild, &text, &hash, &mut progress).await {
        Ok(report) => {
            if json {
                render::json(out, &report);
            } else {
                render::apply_report(err, &report);
            }
            if report.is_complete() { Exit::Ok } else { Exit::Error }
        }
        Err(e) => render::service_error(err, &e),
    }
}
