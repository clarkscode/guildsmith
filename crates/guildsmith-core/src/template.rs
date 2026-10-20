//! Template import (FR-035/044, research E12): a server template's `serialized_source_guild` →
//! a `GuildSpec`. Pure.
//!
//! Templates use small placeholder integers as ids. They are mapped to a synthetic snapshot,
//! exported the same way `export` does, and the ids are then dropped, so overwrites name roles
//! and every item is matched by name when the file is planned against a real server. No server
//! is ever created: bots can't create servers.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

use crate::export::{self, ExportNotes};
use crate::id::Id;
use crate::permissions::PermissionSet;
use crate::snapshot::{
    BotInfo, Completeness, LiveChannel, LiveChannelKind, LiveGuild, LiveOverwrite, LiveRole, LiveTag, OverwriteKind,
    Snapshot,
};
use crate::spec::{ChannelKind, GuildSpec, Target};

/// Placeholder ids are offset so roles and channels can't collide (and role 0, `@everyone`,
/// becomes the guild id).
const ROLE_BASE: u64 = 1;
const CHANNEL_BASE: u64 = 1_000_000;

/// A template that can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("this template can't be read: {0}")]
pub struct TemplateError(pub String);

fn placeholder<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n.as_u64().ok_or_else(|| serde::de::Error::custom("negative id")),
        Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!("bad id {other}"))),
    }
}

fn opt_placeholder<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_u64()),
        Some(Value::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!("bad id {other}"))),
    }
}

fn perms<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PermissionSet, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => PermissionSet::from_discord_str(&s).map_err(serde::de::Error::custom),
        Value::Number(n) => Ok(PermissionSet::from_bits(n.as_u64().unwrap_or(0))),
        _ => Ok(PermissionSet::EMPTY),
    }
}

#[derive(Debug, Deserialize)]
struct Role {
    #[serde(deserialize_with = "placeholder")]
    id: u64,
    name: String,
    #[serde(default)]
    color: u32,
    #[serde(default)]
    hoist: bool,
    #[serde(default)]
    mentionable: bool,
    #[serde(default, deserialize_with = "perms")]
    permissions: PermissionSet,
}

#[derive(Debug, Deserialize)]
struct Overwrite {
    #[serde(deserialize_with = "placeholder")]
    id: u64,
    #[serde(rename = "type", default)]
    kind: Value,
    #[serde(default, deserialize_with = "perms")]
    allow: PermissionSet,
    #[serde(default, deserialize_with = "perms")]
    deny: PermissionSet,
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
    #[serde(default)]
    emoji_name: Option<String>,
    #[serde(default)]
    moderated: bool,
}

#[derive(Debug, Deserialize)]
struct Reaction {
    #[serde(default)]
    emoji_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Channel {
    #[serde(deserialize_with = "placeholder")]
    id: u64,
    #[serde(rename = "type")]
    kind: u8,
    name: String,
    #[serde(default)]
    position: i32,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    bitrate: Option<u32>,
    #[serde(default)]
    user_limit: Option<u32>,
    #[serde(default)]
    nsfw: bool,
    #[serde(default)]
    rate_limit_per_user: u32,
    #[serde(default, deserialize_with = "opt_placeholder")]
    parent_id: Option<u64>,
    #[serde(default)]
    permission_overwrites: Vec<Overwrite>,
    #[serde(default)]
    available_tags: Option<Vec<Tag>>,
    #[serde(default)]
    default_reaction_emoji: Option<Reaction>,
}

/// The parts of `serialized_source_guild` guildsmith reads.
#[derive(Debug, Deserialize)]
pub struct SourceGuild {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub verification_level: Option<u8>,
    #[serde(default)]
    pub default_message_notifications: Option<u8>,
    #[serde(default)]
    pub explicit_content_filter: Option<u8>,
    #[serde(default)]
    pub preferred_locale: Option<String>,
    #[serde(default)]
    pub afk_timeout: Option<u32>,
    #[serde(default, deserialize_with = "opt_placeholder")]
    pub afk_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "opt_placeholder")]
    pub system_channel_id: Option<u64>,
    #[serde(default)]
    pub system_channel_flags: Option<u64>,
    #[serde(default)]
    roles: Vec<Role>,
    #[serde(default)]
    channels: Vec<Channel>,
}

impl SourceGuild {
    /// The name a placeholder channel id has in the template.
    #[must_use]
    pub fn channel_name(&self, placeholder: u64) -> Option<&str> {
        self.channels.iter().find(|c| c.id == placeholder).map(|c| c.name.as_str())
    }
}

fn kind(t: u8) -> LiveChannelKind {
    match t {
        0 => LiveChannelKind::Known(ChannelKind::Text),
        2 => LiveChannelKind::Known(ChannelKind::Voice),
        4 => LiveChannelKind::Category,
        5 => LiveChannelKind::Known(ChannelKind::Announcement),
        13 => LiveChannelKind::Known(ChannelKind::Stage),
        15 => LiveChannelKind::Known(ChannelKind::Forum),
        16 => LiveChannelKind::Known(ChannelKind::Media),
        other => LiveChannelKind::Other(other),
    }
}

/// The synthetic snapshot a template describes.
fn snapshot(g: &SourceGuild) -> Snapshot {
    let guild = Id(ROLE_BASE);
    let roles: Vec<LiveRole> = g
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| LiveRole {
            id: Id(ROLE_BASE + r.id),
            name: r.name.clone(),
            color: r.color,
            secondary_color: None,
            tertiary_color: None,
            hoist: r.hoist,
            mentionable: r.mentionable,
            managed: false,
            permissions: r.permissions,
            // Templates list roles lowest first, @everyone at 0.
            position: i64::try_from(i).unwrap_or(i64::MAX),
            tags: None,
        })
        .collect();
    let channels = g
        .channels
        .iter()
        .map(|c| LiveChannel {
            id: Id(CHANNEL_BASE + c.id),
            name: c.name.clone(),
            kind: kind(c.kind),
            parent_id: c.parent_id.map(|p| Id(CHANNEL_BASE + p)),
            position: c.position,
            topic: c.topic.clone().filter(|t| !t.is_empty()),
            nsfw: c.nsfw,
            slowmode: c.rate_limit_per_user,
            bitrate: c.bitrate,
            user_limit: c.user_limit,
            rtc_region: None,
            tags: c
                .available_tags
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, t)| LiveTag {
                    id: Id(u64::try_from(i).unwrap_or(0) + 1),
                    name: t.name.clone(),
                    emoji: t.emoji_name.clone(),
                    moderated: t.moderated,
                })
                .collect(),
            default_reaction: c.default_reaction_emoji.as_ref().and_then(|r| r.emoji_name.clone()),
            overwrites: c
                .permission_overwrites
                .iter()
                .map(|o| LiveOverwrite {
                    id: Id(ROLE_BASE + o.id),
                    // Templates hold role overwrites only (type 0 or "role").
                    kind: if o.kind.as_u64() == Some(1) || o.kind.as_str() == Some("member") {
                        OverwriteKind::Member
                    } else {
                        OverwriteKind::Role
                    },
                    allow: o.allow,
                    deny: o.deny,
                })
                .filter(|o| o.kind == OverwriteKind::Role)
                .collect(),
        })
        .collect();
    Snapshot {
        guild: LiveGuild {
            id: guild,
            name: g.name.clone(),
            owner_id: Id(0),
            features: BTreeSet::new(),
            premium_tier: 0,
        },
        roles,
        channels,
        bot: BotInfo {
            user_id: Id(0),
            role_ids: Vec::new(),
            top_role_position: i64::MAX,
            permissions: PermissionSet::all_known(),
        },
        completeness: Completeness::Complete,
    }
}

/// Reads a template's `serialized_source_guild`.
pub fn source(serialized: &Value) -> Result<SourceGuild, TemplateError> {
    SourceGuild::deserialize(serialized).map_err(|e| TemplateError(e.to_string()))
}

/// The spec a template describes, with no ids, plus notes for the writer (e.g. channel kinds
/// guildsmith doesn't manage).
pub fn import(serialized: &Value) -> Result<(GuildSpec, ExportNotes), TemplateError> {
    let g = source(serialized)?;
    let s = snapshot(&g);
    let (mut spec, notes) = export::to_spec(&s);
    let role_name = |id: Id| s.role(id).map(|r| r.name.clone());
    let fix = |list: &mut Option<Vec<crate::spec::OverwriteSpec>>| {
        for o in list.iter_mut().flatten() {
            if let Target::RoleId(id) = o.target
                && let Some(n) = role_name(id)
            {
                o.target = Target::RoleName(n);
            }
        }
    };
    spec.guild.id = None;
    for r in &mut spec.roles {
        r.id = None;
    }
    for c in &mut spec.categories {
        c.id = None;
        fix(&mut c.overwrites);
        for ch in &mut c.channels {
            ch.id = None;
            fix(&mut ch.overwrites);
            for t in ch.tags.iter_mut().flatten() {
                t.id = None;
            }
        }
    }
    for ch in &mut spec.channels {
        ch.id = None;
        fix(&mut ch.overwrites);
        for t in ch.tags.iter_mut().flatten() {
            t.id = None;
        }
    }
    let unmanaged = notes.unmanaged.iter().map(|u| strip_ids(u)).collect();
    Ok((spec, ExportNotes { unmanaged, ..notes }))
}

/// Notes name synthetic ids; drop them.
fn strip_ids(note: &str) -> String {
    note.split(", id ").next().unwrap_or(note).to_owned()
}
