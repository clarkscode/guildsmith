//! The `expressions` group (US6, FR-031): custom emoji, stickers and soundboard sounds, with
//! approximate slot counts for the server's boost level.
//!
//! Files are checked for type, size and (sounds) length before upload, naming the limit.
//! Slot limits come from support articles, not the API (research E4): they are shown as
//! "approximately", and Discord's own error decides.

use guildsmith_core::Id;
use guildsmith_core::asset::{Asset, AssetKind, FileType};
use guildsmith_core::snapshot::Snapshot;
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::assets::{self, Caller};
use crate::confirm::{self, Outcome};
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, ExpressionEdit, ExpressionInfo, ExpressionKind, NewExpression};
use crate::preflight;
use crate::resolve;
use crate::services::live::{Deleted, invalid, refuse_if, resolved, role_name, server, writable};

/// Approximate slots per boost tier (0–3): emoji (static and animated each), stickers, sounds.
const EMOJI_SLOTS: [usize; 4] = [50, 100, 150, 250];
const STICKER_SLOTS: [usize; 4] = [5, 15, 30, 60];
const SOUND_SLOTS: [usize; 4] = [8, 24, 36, 48];

/// The approximate number of slots for a kind at a boost tier.
#[must_use]
pub fn approx_slots(kind: ExpressionKind, tier: u8) -> usize {
    let i = usize::from(tier.min(3));
    match kind {
        ExpressionKind::Emoji => EMOJI_SLOTS[i],
        ExpressionKind::Sticker => STICKER_SLOTS[i],
        ExpressionKind::Sound => SOUND_SLOTS[i],
    }
}

/// An expression as tools return it. `name`, `description` and `tags` are user-written.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ExpressionRow {
    pub kind: ExpressionKind,
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub animated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

fn row(s: &Snapshot, e: &ExpressionInfo) -> ExpressionRow {
    ExpressionRow {
        kind: e.kind,
        id: e.id,
        name: e.name.clone(),
        animated: e.animated,
        roles: e.roles.iter().map(|r| role_name(s, *r)).collect(),
        description: e.description.clone().filter(|d| !d.is_empty()),
        tags: e.tags.clone(),
        volume: e.volume,
        emoji: e.emoji.clone(),
    }
}

/// Slots used and the approximate maximum for one kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Slots {
    /// `emoji (static)`, `emoji (animated)`, `stickers` or `sounds`.
    pub kind: String,
    pub used: usize,
    /// Approximate: from support articles; Discord's own limit wins.
    pub approx_max: usize,
    pub approx_remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ExpressionsList {
    pub items: Vec<ExpressionRow>,
    pub slots: Vec<Slots>,
    pub note: String,
}

fn slots(label: &str, used: usize, max: usize) -> Slots {
    Slots { kind: label.to_owned(), used, approx_max: max, approx_remaining: max.saturating_sub(used) }
}

/// Emoji, stickers and/or sounds, with slots used and approximately remaining.
pub async fn expressions_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    kind: Option<ExpressionKind>,
) -> Result<ExpressionsList, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let tier = s.guild.premium_tier;
    let kinds: Vec<ExpressionKind> =
        kind.map_or_else(|| vec![ExpressionKind::Emoji, ExpressionKind::Sticker, ExpressionKind::Sound], |k| vec![k]);
    let mut items = Vec::new();
    let mut counts = Vec::new();
    for k in kinds {
        let list = app.port.expressions(gid, k).await?;
        let max = approx_slots(k, tier);
        match k {
            ExpressionKind::Emoji => {
                let animated = list.iter().filter(|e| e.animated).count();
                counts.push(slots("emoji (static)", list.len() - animated, max));
                counts.push(slots("emoji (animated)", animated, max));
            }
            ExpressionKind::Sticker => counts.push(slots("stickers", list.len(), max)),
            ExpressionKind::Sound => counts.push(slots("sounds", list.len(), max)),
        }
        items.extend(list.iter().map(|e| row(&s, e)));
    }
    Ok(ExpressionsList {
        items,
        slots: counts,
        note: format!("slot counts are approximately right for boost tier {tier}; Discord's own limit wins"),
    })
}

fn check_name(kind: ExpressionKind, name: &str) -> Result<(), ServiceError> {
    let n = name.chars().count();
    match kind {
        ExpressionKind::Emoji => {
            if !(2..=32).contains(&n) || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(invalid("emoji names are 2–32 characters: letters, digits and _"));
            }
        }
        ExpressionKind::Sticker => {
            if !(2..=30).contains(&n) {
                return Err(invalid("sticker names are 2–30 characters"));
            }
        }
        ExpressionKind::Sound => {
            if !(2..=32).contains(&n) {
                return Err(invalid("sound names are 2–32 characters"));
            }
        }
    }
    Ok(())
}

fn check_sticker_text(description: Option<&str>, tags: Option<&str>) -> Result<(), ServiceError> {
    if description.is_some_and(|d| !d.is_empty() && !(2..=100).contains(&d.chars().count())) {
        return Err(invalid("a sticker description is empty or 2–100 characters"));
    }
    if tags.is_some_and(|t| t.chars().count() > 200) {
        return Err(invalid("sticker tags are at most 200 characters"));
    }
    Ok(())
}

fn check_volume(v: Option<f64>) -> Result<(), ServiceError> {
    if v.is_some_and(|v| !(0.0..=1.0).contains(&v)) { Err(invalid("volume is 0–1")) } else { Ok(()) }
}

/// What to upload.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionInput {
    pub kind: ExpressionKind,
    pub name: String,
    pub asset: Asset,
    /// Emoji: roles allowed to use it.
    pub roles: Vec<String>,
    /// Stickers.
    pub description: Option<String>,
    /// Stickers: a related emoji name or words (≤ 200 characters).
    pub tags: Option<String>,
    /// Sounds: 0–1.
    pub volume: Option<f64>,
    /// Sounds: a unicode emoji or a custom emoji id.
    pub emoji: Option<String>,
}

/// Uploads an emoji, sticker or sound. The file is checked first: emoji "256 KiB,
/// JPEG/PNG/GIF/WebP/AVIF", stickers "512 KiB, PNG/APNG/GIF" (Lottie only on verified or
/// partnered servers), sounds "MP3/Ogg, 512 KB, ≤ 5.2 s".
pub async fn expression_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    input: &ExpressionInput,
    reason: Option<&str>,
) -> Result<ExpressionRow, ServiceError> {
    check_name(input.kind, input.name.trim())?;
    check_sticker_text(input.description.as_deref(), input.tags.as_deref())?;
    check_volume(input.volume)?;
    let (gid, s) = writable(app, guild).await?;
    let roles =
        input.roles.iter().map(|r| resolved(resolve::resolve_role(&s, r), "role", r)).collect::<Result<Vec<_>, _>>()?;
    if !roles.is_empty() && input.kind != ExpressionKind::Emoji {
        return Err(invalid("only emoji can be limited to roles"));
    }
    let asset_kind = match input.kind {
        ExpressionKind::Emoji => AssetKind::Emoji,
        ExpressionKind::Sticker => AssetKind::Sticker,
        ExpressionKind::Sound => AssetKind::Sound,
    };
    let loaded = assets::load(&app.port, &app.policy, Caller::Mcp { base: None }, &input.asset, asset_kind).await?;
    if loaded.file_type == FileType::Lottie && !s.guild.features.iter().any(|f| f == "VERIFIED" || f == "PARTNERED") {
        return Err(invalid("Lottie stickers need a verified or partnered server; use PNG, APNG or GIF"));
    }
    if input.kind == ExpressionKind::Sticker && input.tags.as_deref().is_none_or(|t| t.trim().is_empty()) {
        return Err(invalid("stickers need `tags`: an emoji or words for autocomplete (≤ 200 characters)"));
    }
    refuse_if(&preflight::check_tools(&s, ["expression_create"]))?;
    let new = NewExpression {
        kind: input.kind,
        name: input.name.trim().to_owned(),
        bytes: loaded.bytes,
        file_type: loaded.file_type,
        roles,
        description: input.description.clone().unwrap_or_default(),
        tags: input.tags.clone().unwrap_or_default(),
        volume: input.volume,
        emoji: input.emoji.clone(),
    };
    let what = format!("add {} {}", input.kind.name(), new.name);
    let e = app.port.create_expression(gid, &new, &audit_reason(&what, reason)).await?;
    Ok(row(&s, &e))
}

async fn find<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    kind: ExpressionKind,
    input: &str,
) -> Result<ExpressionInfo, ServiceError> {
    let all = app.port.expressions(gid, kind).await?;
    let raw = input.trim().trim_matches(':');
    let raw = raw.rsplit(':').next().filter(|r| r.parse::<Id>().is_ok()).unwrap_or(raw).trim_end_matches('>');
    if let Ok(id) = raw.parse::<Id>()
        && let Some(e) = all.iter().find(|e| e.id == id)
    {
        return Ok(e.clone());
    }
    let hits: Vec<&ExpressionInfo> = all.iter().filter(|e| e.name == raw).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(ServiceError::NotFound {
            what: kind.name(),
            input: input.to_owned(),
            suggestions: guildsmith_core::suggest::closest(raw, all.iter().map(|e| e.name.as_str()), 3),
        }),
        many => Err(ServiceError::Ambiguous {
            what: kind.name(),
            input: input.to_owned(),
            candidates: many
                .iter()
                .map(|e| resolve::Candidate { name: e.name.clone(), id: e.id, parent: None })
                .collect(),
        }),
    }
}

/// Changes to an expression. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExpressionUpdate {
    pub name: Option<String>,
    pub roles: Option<Vec<String>>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub volume: Option<f64>,
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ExpressionUpdated {
    pub item: ExpressionRow,
    /// What changed; empty when nothing needed to.
    pub changes: Vec<String>,
}

/// Renames an emoji, sticker or sound, or changes its roles, tags, description, volume or emoji.
pub async fn expression_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    kind: ExpressionKind,
    item: &str,
    input: &ExpressionUpdate,
    reason: Option<&str>,
) -> Result<ExpressionUpdated, ServiceError> {
    if let Some(n) = &input.name {
        check_name(kind, n.trim())?;
    }
    check_sticker_text(input.description.as_deref(), input.tags.as_deref())?;
    check_volume(input.volume)?;
    let (gid, s) = writable(app, guild).await?;
    let e = find(app, gid, kind, item).await?;
    let mut edit = ExpressionEdit::default();
    let mut changes = Vec::new();
    if let Some(n) = input.name.as_ref().map(|n| n.trim().to_owned()).filter(|n| *n != e.name) {
        changes.push(format!("name {} → {n}", e.name));
        edit.name = Some(n);
    }
    if let Some(r) = &input.roles {
        if kind != ExpressionKind::Emoji {
            return Err(invalid("only emoji can be limited to roles"));
        }
        let ids = r.iter().map(|x| resolved(resolve::resolve_role(&s, x), "role", x)).collect::<Result<Vec<_>, _>>()?;
        if ids != e.roles {
            changes.push("roles".to_owned());
            edit.roles = Some(ids);
        }
    }
    if let Some(d) = input.description.as_ref().filter(|d| Some(*d) != e.description.as_ref()) {
        changes.push("description".to_owned());
        edit.description = Some(d.clone());
    }
    if let Some(t) = input.tags.as_ref().filter(|t| Some(*t) != e.tags.as_ref()) {
        changes.push("tags".to_owned());
        edit.tags = Some(t.clone());
    }
    if let Some(v) = input.volume.filter(|v| Some(*v) != e.volume) {
        changes.push(format!("volume → {v}"));
        edit.volume = Some(v);
    }
    if let Some(em) = input.emoji.as_ref().filter(|em| Some(*em) != e.emoji.as_ref()) {
        changes.push("emoji".to_owned());
        edit.emoji = Some((!em.is_empty()).then(|| em.clone()));
    }
    if changes.is_empty() {
        return Ok(ExpressionUpdated { item: row(&s, &e), changes });
    }
    refuse_if(&preflight::check_tools(&s, ["expression_update"]))?;
    let summary = format!("update {} {}: {}", kind.name(), e.name, changes.join(", "));
    let updated = app.port.update_expression(gid, kind, e.id, &edit, &audit_reason(&summary, reason)).await?;
    Ok(ExpressionUpdated { item: row(&s, &updated), changes })
}

/// Deletes an emoji, sticker or sound after confirmation.
pub async fn expression_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    kind: ExpressionKind,
    item: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let e = find(app, gid, kind, item).await?;
    let what = format!("{} {} ({})", kind.name(), e.name, e.id);
    confirm::guarded(confirmed, vec![format!("delete {what}")], || async {
        refuse_if(&preflight::check_tools(&s, ["expression_delete"]))?;
        app.port.delete_expression(gid, kind, e.id, &audit_reason(&format!("delete {what}"), reason)).await?;
        Ok(Deleted { deleted: what.clone() })
    })
    .await
}
