//! The spec file (`guild.toml`): the desired structure of one server (data-model §1).
//!
//! An attribute set to `None` is *not managed*: the planner leaves the live value unchanged.

mod source;
pub mod writer;

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::id::Id;
use crate::permissions::PermissionSet;

pub use source::{Location, ParseError, PathSeg, SpecPath, SpecSource, parse};

// serde's `skip_serializing_if` always passes a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn perms_empty(p: &PermissionSet) -> bool {
    p.is_empty()
}

/// The whole spec file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuildSpec {
    /// Server information. Informational only in v1 (research R21).
    #[serde(default)]
    pub guild: GuildInfo,
    /// Roles, listed highest first. `@everyone` may appear (permissions only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleSpec>,
    /// Categories with their channels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<CategorySpec>,
    /// Top-level channels with no category.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelSpec>,
    /// Which unmanaged kinds may be deleted. Every flag defaults to `false`.
    #[serde(default)]
    pub prune: PruneSpec,
}

/// `[guild]`: informational unless `manage_settings = true` (rejected in v1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuildInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Reserved. v1 rejects `true` (research R21).
    #[serde(default, skip_serializing_if = "is_false")]
    pub manage_settings: bool,
}

/// `[[roles]]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleSpec {
    /// Written by export. Without it, the role is matched by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters. `@everyone` is reserved for the default role.
    pub name: String,
    /// Primary color as `#RRGGBB` (research R2: gradients are read-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mentionable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionSet>,
    /// Optional explicit position. Without it, file order is used (first = highest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u16>,
    /// Must match the live role. If `true`, only `permissions` may differ (research R12).
    #[serde(default, skip_serializing_if = "is_false")]
    pub managed: bool,
}

/// `[[categories]]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CategorySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u16>,
    /// Permission overwrites. Omitted means not managed; `[]` means none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrites: Option<Vec<OverwriteSpec>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelSpec>,
}

/// A channel, either inside `[[categories.channels]]` or in top-level `[[channels]]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters. Text-like kinds are lowercased by Discord.
    pub name: String,
    pub kind: ChannelKind,
    /// 0–1024 characters, or 0–4096 for forum/media. Not allowed on voice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    /// Seconds, 0–21600. Not allowed on announcement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowmode: Option<u32>,
    /// Voice/stage only, ≥ 8000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    /// Voice: 0–99. Stage: ≤ 10000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_limit: Option<u32>,
    /// Voice/stage only. `"auto"` means Discord picks the region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtc_region: Option<String>,
    /// Forum/media only. At most 20; names ≤ 20 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ForumTag>>,
    /// Forum/media only: a unicode emoji or a custom emoji id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reaction: Option<String>,
    /// If `true`, the channel's overwrites are the category's, and `overwrites` must be empty.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sync_permissions: bool,
    /// Permission overwrites. Omitted means not managed; `[]` means none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrites: Option<Vec<OverwriteSpec>>,
    /// Allows a kind change by delete + create (research R10).
    #[serde(default, skip_serializing_if = "is_false")]
    pub replace: bool,
}

/// Channel kinds a spec can describe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    #[default]
    Text,
    Announcement,
    Voice,
    Stage,
    Forum,
    Media,
}

impl ChannelKind {
    /// Text-like kinds, whose names Discord lowercases.
    #[must_use]
    pub const fn is_text_like(self) -> bool {
        matches!(self, ChannelKind::Text | ChannelKind::Announcement | ChannelKind::Forum | ChannelKind::Media)
    }

    #[must_use]
    pub const fn is_voice_like(self) -> bool {
        matches!(self, ChannelKind::Voice | ChannelKind::Stage)
    }

    #[must_use]
    pub const fn is_forum_like(self) -> bool {
        matches!(self, ChannelKind::Forum | ChannelKind::Media)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Text => "text",
            ChannelKind::Announcement => "announcement",
            ChannelKind::Voice => "voice",
            ChannelKind::Stage => "stage",
            ChannelKind::Forum => "forum",
            ChannelKind::Media => "media",
        }
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A forum or media tag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForumTag {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// ≤ 20 characters.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderated: Option<bool>,
}

/// One permission overwrite on a category or channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverwriteSpec {
    pub target: Target,
    #[serde(default, skip_serializing_if = "perms_empty")]
    pub allow: PermissionSet,
    #[serde(default, skip_serializing_if = "perms_empty")]
    pub deny: PermissionSet,
}

/// Who an overwrite applies to: `@everyone`, `@RoleName`, `role:<id>` or `member:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Everyone,
    RoleName(String),
    RoleId(Id),
    Member(Id),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Everyone => f.write_str("@everyone"),
            Target::RoleName(n) => write!(f, "@{n}"),
            Target::RoleId(id) => write!(f, "role:{id}"),
            Target::Member(id) => write!(f, "member:{id}"),
        }
    }
}

/// Error for an overwrite target that is not one of the four forms.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a target (expected `@everyone`, `@RoleName`, `role:<id>` or `member:<id>`)")]
pub struct ParseTargetError(pub String);

impl FromStr for Target {
    type Err = ParseTargetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseTargetError(s.to_owned());
        if s == "@everyone" {
            Ok(Target::Everyone)
        } else if let Some(name) = s.strip_prefix('@') {
            if name.is_empty() { Err(err()) } else { Ok(Target::RoleName(name.to_owned())) }
        } else if let Some(id) = s.strip_prefix("role:") {
            id.parse().map(Target::RoleId).map_err(|_| err())
        } else if let Some(id) = s.strip_prefix("member:") {
            id.parse().map(Target::Member).map_err(|_| err())
        } else {
            Err(err())
        }
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Target {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Target".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^(@.+|role:[0-9]{1,20}|member:[0-9]{1,20})$",
            "description": "`@everyone`, `@RoleName`, `role:<id>` or `member:<id>`."
        })
    }
}

/// A role color, written `#RRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:06X}", self.0)
    }
}

/// Error for a color that is not `#RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a color (expected `#RRGGBB`)")]
pub struct ParseColorError(pub String);

impl FromStr for Color {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').filter(|h| h.len() == 6).ok_or_else(|| ParseColorError(s.to_owned()))?;
        u32::from_str_radix(hex, 16).map(Color).map_err(|_| ParseColorError(s.to_owned()))
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "pattern": "^#[0-9A-Fa-f]{6}$" })
    }
}

/// `[prune]`: which unmanaged kinds may be deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PruneSpec {
    #[serde(default)]
    pub roles: bool,
    #[serde(default)]
    pub categories: bool,
    #[serde(default)]
    pub channels: bool,
}

impl GuildSpec {
    /// Every channel with the category it belongs to (`None` = top level), in file order.
    pub fn all_channels(&self) -> impl Iterator<Item = (Option<&CategorySpec>, &ChannelSpec)> {
        self.categories
            .iter()
            .flat_map(|c| c.channels.iter().map(move |ch| (Some(c), ch)))
            .chain(self.channels.iter().map(|ch| (None, ch)))
    }
}

/// The JSON Schema (draft 2020-12) for spec files, as published in `schema/guild.schema.json`.
#[must_use]
pub fn json_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(GuildSpec)).expect("schema serializes");
    if let Some(obj) = schema.as_object_mut() {
        obj.insert("$id".to_owned(), serde_json::Value::String(writer::SCHEMA_URL.to_owned()));
        obj.insert("title".to_owned(), serde_json::Value::String("guildsmith spec file (guild.toml)".to_owned()));
    }
    schema
}

/// `json_schema()` as pretty JSON with a trailing newline (the committed file's exact bytes).
#[must_use]
pub fn json_schema_text() -> String {
    let mut text = serde_json::to_string_pretty(&json_schema()).expect("schema serializes");
    text.push('\n');
    text
}
