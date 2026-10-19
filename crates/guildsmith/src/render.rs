//! Human-readable output for CLI commands. `--json` output uses the same structures as the MCP
//! tools' `structuredContent`.

use std::io::Write;

use guildsmith_app::error::ServiceError;
use guildsmith_app::services::apply::ApplyReport;
use guildsmith_app::services::doctor::{DoctorReport, Status};
use guildsmith_app::services::guilds::GuildList;
use guildsmith_app::services::plan::PlanOutput;
use guildsmith_core::planner::Risk;
use guildsmith_core::validate::{Problem, Severity};
use serde::Serialize;

use crate::cli::Exit;

/// Writes `value` as pretty JSON.
pub fn json<T: Serialize>(out: &mut dyn Write, value: &T) {
    let _ = writeln!(out, "{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

/// Prints a service error and returns the matching exit code: refusals → 3, the rest → 1.
pub fn service_error(err: &mut dyn Write, e: &ServiceError) -> Exit {
    match e {
        ServiceError::Refused(r) => {
            let _ = writeln!(err, "refused: {}", r.reason);
            if let Some(h) = &r.hint {
                let _ = writeln!(err, "hint: {h}");
            }
            Exit::Refused
        }
        other => {
            let _ = writeln!(err, "error: {other}");
            Exit::Error
        }
    }
}

pub fn guilds(out: &mut dyn Write, list: &GuildList) {
    if list.guilds.is_empty() {
        let _ = writeln!(out, "No servers available.");
    }
    let width = list.guilds.iter().map(|g| g.id.to_string().len()).max().unwrap_or(2);
    for g in &list.guilds {
        let _ = writeln!(out, "{:<width$}  {}", g.id, g.name);
    }
    if let Some(h) = &list.hint {
        let _ = writeln!(out, "hint: {h}");
    }
}

/// `file:line:col: severity: message`, plus a hint line.
pub fn problems(out: &mut dyn Write, file: &str, problems: &[Problem]) {
    for p in problems {
        let sev = match p.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let at = p.location.map_or_else(|| file.to_owned(), |l| format!("{file}:{l}"));
        let _ = writeln!(out, "{at}: {sev}: {}", p.message);
        if let Some(h) = &p.hint {
            let _ = writeln!(out, "  hint: {h}");
        }
    }
}

fn risk_word(r: Risk) -> &'static str {
    match r {
        Risk::Safe => "safe",
        Risk::Destructive => "destructive",
    }
}

/// The plan: ops in order with their risk, unmanaged items, warnings, estimate and hash.
pub fn plan(out: &mut dyn Write, p: &PlanOutput) {
    if p.has_changes() {
        let _ = writeln!(out, "Plan: {} change(s), {}", p.ops.len(), risk_word(p.risk));
        let width = p.ops.len().to_string().len();
        for (i, op) in p.ops.iter().enumerate() {
            let _ = writeln!(out, "  {:>width$}. [{}] {}", i + 1, risk_word(op.risk), op.summary);
        }
    } else {
        let _ = writeln!(out, "No changes.");
    }
    if !p.unmanaged.is_empty() {
        let _ = writeln!(out, "Unmanaged (left alone):");
        for u in &p.unmanaged {
            let _ = writeln!(out, "  - {u}");
        }
    }
    for w in &p.warnings {
        let _ = writeln!(out, "warning: {w}");
    }
    if p.has_changes() {
        let _ = writeln!(out, "Estimated requests: {}", p.estimated_requests);
    }
    let _ = writeln!(out, "Plan hash: {}", p.plan_hash);
}

/// What an apply did: everything applied, or where it stopped.
pub fn apply_report(out: &mut dyn Write, r: &ApplyReport) {
    match &r.failed {
        None => {
            let _ = writeln!(out, "Applied {} change(s).", r.applied.len());
        }
        Some(f) => {
            let _ = writeln!(out, "error: apply stopped at `{}`: {}", f.summary, f.error);
            let _ = writeln!(out, "Applied before the error ({}):", r.applied.len());
            for a in &r.applied {
                let _ = writeln!(out, "  - {a}");
            }
            let _ = writeln!(out, "Not attempted ({}):", r.skipped.len());
            for s in &r.skipped {
                let _ = writeln!(out, "  - {s}");
            }
            let _ = writeln!(out, "hint: run plan again; it lists only what's left");
        }
    }
}

/// Each doctor check with its status, then every problem with its fix.
pub fn doctor(out: &mut dyn Write, r: &DoctorReport) {
    for c in &r.checks {
        let mark = match c.status {
            Status::Pass => "ok  ",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
            Status::Skipped => "skip",
        };
        let _ = writeln!(out, "[{mark}] {}: {}", c.name, c.detail);
        for p in &c.problems {
            let _ = writeln!(out, "       - {}", p.message);
            let _ = writeln!(out, "         fix: {}", p.fix);
        }
    }
    let _ = writeln!(out, "{}", if r.ok { "All checks passed." } else { "Some checks failed." });
}
