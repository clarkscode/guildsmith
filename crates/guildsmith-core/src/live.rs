//! Live state of the kinds a spec file can manage beyond roles and channels (US9): what the
//! group ports return, and what the planner compares a spec with. Pure data.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::Id;
use crate::spec::automod::AutomodRule;

/// Server settings as Discord holds them. Images are hashes (`None` = none).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GuildSettings {
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub splash: Option<String>,
    pub system_channel_id: Option<Id>,
    pub system_channel_flags: u64,
    pub rules_channel_id: Option<Id>,
    pub public_updates_channel_id: Option<Id>,
    pub safety_alerts_channel_id: Option<Id>,
    pub afk_channel_id: Option<Id>,
    pub afk_timeout: u32,
    pub verification_level: u8,
    pub default_notifications: u8,
    pub explicit_content_filter: u8,
    pub preferred_locale: String,
    pub premium_progress_bar: bool,
    pub features: BTreeSet<String>,
}

/// One onboarding option. `title` and `description` are admin-written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OnboardingOption {
    pub id: Id,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub role_ids: Vec<Id>,
    pub channel_ids: Vec<Id>,
}

/// One onboarding question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OnboardingPrompt {
    pub id: Id,
    pub title: String,
    /// 0 multiple choice, 1 dropdown.
    pub kind: u8,
    pub single_select: bool,
    pub required: bool,
    pub in_onboarding: bool,
    pub options: Vec<OnboardingOption>,
}

/// The server's onboarding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Onboarding {
    pub enabled: bool,
    /// 0 default channels only, 1 advanced (prompts can add channels).
    pub mode: u8,
    pub default_channel_ids: Vec<Id>,
    pub prompts: Vec<OnboardingPrompt>,
}

/// A welcome screen channel. `description` is admin-written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WelcomeChannel {
    pub channel_id: Id,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WelcomeScreen {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub channels: Vec<WelcomeChannel>,
}

/// An invite. `inviter` is a user name: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InviteInfo {
    pub code: String,
    pub channel_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inviter_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inviter: Option<String>,
    /// Seconds; 0 = never expires.
    pub max_age: u32,
    /// 0 = unlimited.
    pub max_uses: u32,
    pub uses: u32,
    pub temporary: bool,
    /// RFC 3339.
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Roles new members get from this invite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<Id>,
}

/// Where an event happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventLocation {
    Voice(Id),
    Stage(Id),
    External(String),
}

/// A scheduled event. `name` and `description` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventInfo {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    /// RFC 3339.
    pub start: String,
    pub end: Option<String>,
    /// `scheduled`, `active`, `completed` or `canceled`.
    pub status: String,
    pub location: EventLocation,
    /// Interested members.
    pub interested: u32,
    pub creator: Option<Id>,
    /// Discord's `recurrence_rule`, as sent.
    pub recurrence: Option<Value>,
    /// The cover image's hash on Discord's CDN.
    #[serde(skip)]
    pub image: Option<String>,
}

/// What made a webhook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookKind {
    /// Posts messages; the only kind guildsmith edits.
    Incoming,
    /// Follows an announcement channel (read-only here).
    ChannelFollower,
    /// Belongs to an application's interactions (read-only here).
    Application,
}

/// A webhook, without its token or URL. `name` is user-chosen: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebhookInfo {
    pub id: Id,
    pub kind: WebhookKind,
    pub name: String,
    pub channel_id: Id,
    pub creator_id: Option<Id>,
    pub creator_name: Option<String>,
    /// For followers: the followed channel's name.
    pub source: Option<String>,
    /// The avatar's hash on Discord's CDN (`None` = the default avatar).
    #[serde(skip)]
    pub avatar: Option<String>,
}

/// Which kind of expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKind {
    Emoji,
    Sticker,
    Sound,
}

impl ExpressionKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ExpressionKind::Emoji => "emoji",
            ExpressionKind::Sticker => "sticker",
            ExpressionKind::Sound => "sound",
        }
    }
}

/// An emoji, sticker or sound. `name`, `description` and `tags` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpressionInfo {
    pub kind: ExpressionKind,
    pub id: Id,
    pub name: String,
    /// Emoji: animated (GIF).
    pub animated: bool,
    /// Emoji: roles allowed to use it (empty = everyone).
    pub roles: Vec<Id>,
    /// Stickers.
    pub description: Option<String>,
    pub tags: Option<String>,
    /// Stickers: 1 PNG, 2 APNG, 3 Lottie, 4 GIF.
    pub format: Option<u8>,
    /// Sounds: 0–1.
    pub volume: Option<f64>,
    /// Sounds: a unicode emoji or a custom emoji id.
    pub emoji: Option<String>,
    /// Where Discord serves the file.
    pub url: String,
}

/// One of the bot's own messages, as the fixed-message planner sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveMessage {
    pub id: Id,
    pub content: String,
    pub embeds: Vec<Value>,
    /// Components V2 layout, if the message has one.
    pub components: Option<Value>,
    pub pinned: bool,
    pub flags: u64,
}

/// Message flag: embeds hidden.
pub const SUPPRESS_EMBEDS: u64 = 1 << 2;
/// Message flag: a Components V2 layout.
pub const COMPONENTS_V2: u64 = 1 << 15;

/// The live state of every kind the spec lists. `None` = not fetched, because the spec doesn't
/// list that kind; the planner leaves it alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LiveExtras {
    pub settings: Option<GuildSettings>,
    pub automod: Option<Vec<AutomodRule<Id>>>,
    pub onboarding: Option<Onboarding>,
    pub welcome_screen: Option<WelcomeScreen>,
    pub expressions: Option<Vec<ExpressionInfo>>,
    pub webhooks: Option<Vec<WebhookInfo>>,
    pub invites: Option<Vec<InviteInfo>>,
    pub events: Option<Vec<EventInfo>>,
    /// Channel → the bot's own recent messages there (at most 100).
    pub messages: Option<BTreeMap<Id, Vec<LiveMessage>>>,
    /// blake3 of live files by asset key (see [`asset_key`]); a missing key wasn't fetched.
    pub asset_hashes: BTreeMap<String, String>,
    /// When this was read, in Unix seconds: past events and expired invites depend on it.
    #[serde(skip)]
    pub now: i64,
}

/// The key of a live file in [`LiveExtras::asset_hashes`]: `settings.icon`, `emoji:<id>`,
/// `webhook:<id>`, `event:<id>`.
#[must_use]
pub fn asset_key(kind: &str, id: impl std::fmt::Display) -> String {
    format!("{kind}:{id}")
}

impl LiveExtras {
    /// Whether nothing beyond v1's structure was read.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == LiveExtras { now: self.now, ..LiveExtras::default() }
    }

    /// blake3 of the parts the planner reads, as hex. Counters that change on their own
    /// (invite uses, interested members) are left out, so they don't make an apply refuse.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut x = self.clone();
        for i in x.invites.iter_mut().flatten() {
            i.uses = 0;
        }
        for e in x.events.iter_mut().flatten() {
            e.interested = 0;
        }
        let mut canonical = serde_json::to_value(&x).unwrap_or(Value::Null);
        // Hashes the CDN serves, not otherwise serialized.
        let images: Vec<_> = x.webhooks.iter().flatten().map(|w| (w.id, w.avatar.clone())).collect();
        let covers: Vec<_> = x.events.iter().flatten().map(|e| (e.id, e.image.clone())).collect();
        canonical["_images"] = serde_json::json!([images, covers]);
        blake3::hash(canonical.to_string().as_bytes()).to_hex().to_string()
    }
}

/// Where Discord's CDN serves a server image (`icons`, `banners`, `splashes`).
#[must_use]
pub fn guild_image_url(folder: &str, guild: Id, hash: &str) -> String {
    let ext = if hash.starts_with("a_") { "gif" } else { "png" };
    format!("https://cdn.discordapp.com/{folder}/{guild}/{hash}.{ext}")
}

/// Where Discord's CDN serves a webhook's avatar.
#[must_use]
pub fn avatar_url(user: Id, hash: &str) -> String {
    let ext = if hash.starts_with("a_") { "gif" } else { "png" };
    format!("https://cdn.discordapp.com/avatars/{user}/{hash}.{ext}")
}

/// Where Discord's CDN serves an event's cover image.
#[must_use]
pub fn event_cover_url(event: Id, hash: &str) -> String {
    format!("https://cdn.discordapp.com/guild-events/{event}/{hash}.png")
}
