//! A normalized view of a server's live structure at one moment (data-model §2).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::Id;
use crate::permissions::PermissionSet;
use crate::spec::ChannelKind;

/// Live server structure, as built by the Discord adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub guild: LiveGuild,
    pub roles: Vec<LiveRole>,
    pub channels: Vec<LiveChannel>,
    pub bot: BotInfo,
    pub completeness: Completeness,
}

/// Server-level facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveGuild {
    pub id: Id,
    pub name: String,
    pub owner_id: Id,
    /// Discord feature flags such as `NEWS`, `COMMUNITY`, `ENHANCED_ROLE_COLORS`.
    pub features: BTreeSet<String>,
    pub premium_tier: u8,
}

/// A role as it exists on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRole {
    pub id: Id,
    pub name: String,
    /// Primary color (`0` = none).
    pub color: u32,
    /// Read-only: gradient colors can't be set through twilight 0.17.1 (research R2).
    pub secondary_color: Option<u32>,
    pub tertiary_color: Option<u32>,
    pub hoist: bool,
    pub mentionable: bool,
    pub managed: bool,
    pub permissions: PermissionSet,
    pub position: i64,
    pub tags: Option<RoleTags>,
}

/// Why a role is managed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleTags {
    pub bot_id: Option<Id>,
    pub integration_id: Option<Id>,
    pub premium_subscriber: bool,
}

/// The kind of a live channel, including categories and kinds a spec can't describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LiveChannelKind {
    Category,
    Known(ChannelKind),
    /// Discord channel type number guildsmith does not manage (e.g. threads, directory).
    Other(u8),
}

/// A channel or category as it exists on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveChannel {
    pub id: Id,
    pub name: String,
    pub kind: LiveChannelKind,
    pub parent_id: Option<Id>,
    pub position: i32,
    pub topic: Option<String>,
    pub nsfw: bool,
    pub slowmode: u32,
    pub bitrate: Option<u32>,
    pub user_limit: Option<u32>,
    /// `None` = automatic.
    pub rtc_region: Option<String>,
    pub tags: Vec<LiveTag>,
    pub default_reaction: Option<String>,
    pub overwrites: Vec<LiveOverwrite>,
}

/// A forum or media tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTag {
    pub id: Id,
    pub name: String,
    pub emoji: Option<String>,
    pub moderated: bool,
}

/// Whether an overwrite targets a role or a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OverwriteKind {
    Role,
    Member,
}

/// A live permission overwrite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveOverwrite {
    pub id: Id,
    pub kind: OverwriteKind,
    pub allow: PermissionSet,
    pub deny: PermissionSet,
}

/// The bot's own standing in the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotInfo {
    pub user_id: Id,
    pub role_ids: Vec<Id>,
    /// Position of the bot's highest role (`0` if it only has `@everyone`).
    pub top_role_position: i64,
    /// Server-level permissions: `@everyone` plus every bot role (all, if owner or admin).
    pub permissions: PermissionSet,
}

/// Whether the snapshot can be trusted to list every channel (research R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Completeness {
    /// The bot is an administrator (or owner) and sees every channel.
    Complete,
    /// The bot may not see private channels; they could be missing.
    MaybeIncomplete,
}

impl Snapshot {
    /// The `@everyone` role, whose id equals the server id.
    #[must_use]
    pub fn everyone(&self) -> Option<&LiveRole> {
        self.roles.iter().find(|r| r.id == self.guild.id)
    }

    #[must_use]
    pub fn role(&self, id: Id) -> Option<&LiveRole> {
        self.roles.iter().find(|r| r.id == id)
    }

    #[must_use]
    pub fn channel(&self, id: Id) -> Option<&LiveChannel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// A copy with every list sorted by id and every overwrite list sorted, so equal servers
    /// compare (and fingerprint) equal regardless of the order Discord returned them in.
    #[must_use]
    pub fn normalized(&self) -> Snapshot {
        let mut s = self.clone();
        s.roles.sort_by_key(|r| r.id);
        s.channels.sort_by_key(|c| c.id);
        for c in &mut s.channels {
            c.overwrites.sort_by_key(|o| (o.kind, o.id));
            c.tags.sort_by_key(|t| t.id);
        }
        s.bot.role_ids.sort();
        s
    }

    /// blake3 of the canonical (normalized) snapshot, as 64 hex characters.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let canonical = serde_json::to_vec(&self.normalized()).expect("snapshot serializes");
        blake3::hash(&canonical).to_hex().to_string()
    }
}
