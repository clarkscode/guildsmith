//! `ExportService`: live server → spec file text (US1).

use std::path::Path;

use guildsmith_core::export::to_spec;
use guildsmith_core::spec::GuildSpec;
use guildsmith_core::spec::writer::{self, Header};
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::DiscordPort;
use crate::services::guilds;

/// The result of an export.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportOutput {
    pub toml: String,
    #[serde(skip)]
    pub spec: GuildSpec,
    pub warnings: Vec<String>,
    /// Live items the spec doesn't describe.
    pub unmanaged: Vec<String>,
}

/// Exports a server. `today` is `YYYY-MM-DD` for the provenance comment.
pub async fn export<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    today: &str,
) -> Result<ExportOutput, ServiceError> {
    let id = guilds::resolve(app, guild).await?;
    let snapshot = app.port.snapshot(id).await?;
    let (spec, notes) = to_spec(&snapshot);
    let header = Header {
        version: env!("CARGO_PKG_VERSION"),
        guild_name: &snapshot.guild.name,
        guild_id: &snapshot.guild.id.to_string(),
        date: today,
    };
    let toml = writer::write(&spec, &header, &notes);
    let mut warnings = Vec::new();
    if notes.incomplete {
        warnings.push("the bot is not an administrator: hidden channels may be missing from this export".to_owned());
    }
    if !notes.managed_roles.is_empty() {
        warnings.push(format!("{} managed role(s) are listed as read-only comments", notes.managed_roles.len()));
    }
    Ok(ExportOutput { toml, spec, warnings, unmanaged: notes.unmanaged })
}

/// Writes export text to a new file. Never overwrites unless `force`.
pub fn write_file(path: &Path, text: &str, force: bool) -> Result<(), ServiceError> {
    if path.exists() && !force {
        return Err(ServiceError::Invalid(format!(
            "{} already exists; refusing to overwrite it (use --force to replace it)",
            path.display()
        )));
    }
    std::fs::write(path, text).map_err(|e| ServiceError::Io(format!("can't write {}: {e}", path.display())))
}
