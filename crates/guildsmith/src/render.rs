//! Human-readable output for CLI commands. `--json` output uses the same structures as the MCP
//! tools' `structuredContent`.

use std::io::Write;

use guildsmith_app::error::ServiceError;
use guildsmith_app::services::guilds::GuildList;
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
