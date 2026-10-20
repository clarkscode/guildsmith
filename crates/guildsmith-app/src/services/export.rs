//! `ExportService`: live server → spec file text (US1).

use std::path::{Path, PathBuf};

use guildsmith_core::asset::{self, Asset};
use guildsmith_core::export::{add_kinds, to_spec};
use guildsmith_core::spec::GuildSpec;
use guildsmith_core::spec::writer::{self, Header};
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::DiscordPort;
use crate::services::{extras, guilds};

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

/// A file name from a user-chosen name: letters, digits, `-` and `_`.
fn file_stem(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.trim_matches('_').is_empty() { "file".to_owned() } else { s }
}

/// Every `url` asset in the spec, with the folder and name to save it under.
fn spec_urls(spec: &mut GuildSpec) -> Vec<(&mut Asset, &'static str, String)> {
    let mut out: Vec<(&mut Asset, &'static str, String)> = Vec::new();
    if let Some(s) = spec.guild.settings.as_mut() {
        for (a, n) in [(&mut s.icon, "icon"), (&mut s.banner, "banner"), (&mut s.splash, "splash")] {
            if let Some(a) = a.as_mut() {
                out.push((a, "guild", n.to_owned()));
            }
        }
    }
    out.extend(spec.emoji.iter_mut().map(|e| (&mut e.image, "emoji", e.name.clone())));
    out.extend(spec.stickers.iter_mut().map(|e| (&mut e.image, "stickers", e.name.clone())));
    out.extend(spec.sounds.iter_mut().map(|e| (&mut e.sound, "sounds", e.name.clone())));
    out.extend(spec.webhooks.iter_mut().filter_map(|w| Some((w.avatar.as_mut()?, "webhooks", w.name.clone()))));
    out.extend(spec.events.iter_mut().filter_map(|e| Some((e.image.as_mut()?, "events", e.name.clone()))));
    out
}

/// Downloads every file into `<dir>/assets/<kind>/<name>.<ext>` and points the spec at it.
async fn save_assets<P: DiscordPort>(
    app: &App<P>,
    spec: &mut GuildSpec,
    dir: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), ServiceError> {
    let mut used = std::collections::BTreeSet::new();
    for (a, folder, name) in spec_urls(spec) {
        let Asset::Url { url } = a else { continue };
        let bytes = match app.port.fetch_url(url, 25 * asset::MIB).await {
            Ok(b) => b,
            Err(e) => {
                warnings.push(format!("couldn't download {folder}/{name}: {e}; it stays a url"));
                continue;
            }
        };
        let mut stem = file_stem(&name);
        while !used.insert(format!("{folder}/{stem}")) {
            stem.push('_');
        }
        let rel = format!("assets/{folder}/{stem}.{}", asset::detect(&bytes).ext());
        let path: PathBuf = dir.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ServiceError::Io(format!("can't create {}: {e}", parent.display())))?;
        }
        std::fs::write(&path, &bytes).map_err(|e| ServiceError::Io(format!("can't write {}: {e}", path.display())))?;
        *a = Asset::File { file: rel };
    }
    Ok(())
}

/// Exports a server. `today` is `YYYY-MM-DD` for the provenance comment. Files stay CDN urls;
/// see [`export_to`].
pub async fn export<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    today: &str,
) -> Result<ExportOutput, ServiceError> {
    export_to(app, guild, today, None).await
}

/// [`export`], every kind included. With `assets_dir`, files are downloaded into
/// `<assets_dir>/assets/<kind>/` and referred to as `file = …` (research E7). Webhook URLs and
/// tokens are never exported (E8).
pub async fn export_to<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    today: &str,
    assets_dir: Option<&Path>,
) -> Result<ExportOutput, ServiceError> {
    let id = guilds::resolve(app, guild).await?;
    let snapshot = app.snapshot(id).await?;
    let (mut spec, notes) = to_spec(&snapshot);
    let (live, mut file_warnings) = extras::read_all(app, id, &snapshot).await;
    add_kinds(&snapshot, &live, &mut spec);
    if let Some(dir) = assets_dir {
        save_assets(app, &mut spec, dir, &mut file_warnings).await?;
    }
    let header = Header {
        version: env!("CARGO_PKG_VERSION"),
        guild_name: &snapshot.guild.name,
        guild_id: &snapshot.guild.id.to_string(),
        date: today,
    };
    let toml = writer::write(&spec, &header, &notes);
    let mut warnings = file_warnings;
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
