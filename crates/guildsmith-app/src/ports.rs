//! Ports: what the application layer needs from the outside world.
//!
//! `DiscordPort` is the only way guildsmith talks to Discord. The twilight adapter and the
//! in-memory `FakeDiscord` both implement it. Every write takes the audit-log `reason`.

use std::future::Future;
use std::time::SystemTime;

use guildsmith_core::Id;
use guildsmith_core::snapshot::{LiveChannelKind, LiveOverwrite, Snapshot};
use guildsmith_core::spec::ForumTag;
use serde::Serialize;

/// The account behind the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrentUser {
    pub id: Id,
    pub username: String,
    /// `false` for user accounts; guildsmith refuses those (FR-022).
    pub bot: bool,
}

/// A server the bot is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuildSummary {
    pub id: Id,
    pub name: String,
}

/// A server member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Member {
    pub id: Id,
    pub username: String,
    /// Global display name.
    pub display_name: Option<String>,
    /// Server nickname.
    pub nick: Option<String>,
    pub role_ids: Vec<Id>,
    pub timed_out: bool,
}

/// A message. `content` and `author_name` are written by users: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub author_id: Id,
    pub author_name: String,
    pub content: String,
    /// RFC 3339 timestamp.
    pub timestamp: String,
}

/// Role attributes to create or update. `None` leaves an attribute unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoleFields {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
    pub permissions: Option<guildsmith_core::PermissionSet>,
}

/// Channel attributes to create or update. `None` leaves an attribute unchanged.
///
/// Overwrites are sent as the channel's full list in the same request as the other fields
/// (research R7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChannelFields {
    pub name: Option<String>,
    /// Required when creating. Changing it is only allowed text ↔ announcement.
    pub kind: Option<LiveChannelKind>,
    /// `Some(None)` moves the channel to the top level.
    pub parent_id: Option<Option<Id>>,
    pub position: Option<i32>,
    pub topic: Option<String>,
    pub nsfw: Option<bool>,
    pub slowmode: Option<u32>,
    pub bitrate: Option<u32>,
    pub user_limit: Option<u32>,
    /// `Some(None)` means automatic.
    pub rtc_region: Option<Option<String>>,
    pub tags: Option<Vec<ForumTag>>,
    pub default_reaction: Option<Option<String>>,
    pub overwrites: Option<Vec<LiveOverwrite>>,
}

/// One entry of a bulk role reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RolePosition {
    pub id: Id,
    pub position: i64,
}

/// One entry of a bulk channel reorder. guildsmith only sends positions here; moving a channel
/// to another category goes through `update_channel`, because Discord allows only one
/// `parent_id` change per bulk request (research R8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChannelPosition {
    pub id: Id,
    pub position: i32,
    pub parent_id: Option<Option<Id>>,
}

/// Errors from Discord (or the fake).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("missing permission: {0}")]
    Forbidden(String),
    #[error("the bot token is invalid or was revoked")]
    Unauthorized,
    #[error("this token does not belong to a bot application; only bot tokens are supported")]
    NotBotToken,
    #[error("no bot token configured; set DISCORD_TOKEN or run `guildsmith auth set`")]
    NoToken,
    #[error("the bot is not in this server ({0})")]
    NotInGuild(Id),
    #[error("Discord error {status}{}: {message}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Discord { status: u16, code: Option<u64>, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
}

impl PortError {
    /// Whether a read may be retried (5xx and network errors).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, PortError::Network(_)) || matches!(self, PortError::Discord { status, .. } if *status >= 500)
    }
}

pub type PortResult<T> = Result<T, PortError>;

/// Everything guildsmith asks of Discord.
pub trait DiscordPort: Send + Sync {
    fn current_user(&self) -> impl Future<Output = PortResult<CurrentUser>> + Send;
    fn list_guilds(&self) -> impl Future<Output = PortResult<Vec<GuildSummary>>> + Send;
    fn snapshot(&self, guild: Id) -> impl Future<Output = PortResult<Snapshot>> + Send;

    fn member(&self, guild: Id, user: Id) -> impl Future<Output = PortResult<Member>> + Send;
    fn search_members(
        &self,
        guild: Id,
        query: &str,
        limit: u16,
    ) -> impl Future<Output = PortResult<Vec<Member>>> + Send;

    fn create_role(&self, guild: Id, fields: &RoleFields, reason: &str) -> impl Future<Output = PortResult<Id>> + Send;
    fn update_role(
        &self,
        guild: Id,
        role: Id,
        fields: &RoleFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn delete_role(&self, guild: Id, role: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    fn reorder_roles(
        &self,
        guild: Id,
        order: &[RolePosition],
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;

    fn create_channel(
        &self,
        guild: Id,
        fields: &ChannelFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<Id>> + Send;
    fn update_channel(
        &self,
        guild: Id,
        channel: Id,
        fields: &ChannelFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn delete_channel(&self, guild: Id, channel: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    fn reorder_channels(
        &self,
        guild: Id,
        positions: &[ChannelPosition],
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;

    fn add_member_role(
        &self,
        guild: Id,
        user: Id,
        role: Id,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn remove_member_role(
        &self,
        guild: Id,
        user: Id,
        role: Id,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;

    fn send_message(&self, channel: Id, content: &str) -> impl Future<Output = PortResult<Message>> + Send;
    fn read_messages(
        &self,
        channel: Id,
        limit: u8,
        before: Option<Id>,
    ) -> impl Future<Output = PortResult<Vec<Message>>> + Send;

    fn ban(
        &self,
        guild: Id,
        user: Id,
        delete_message_seconds: u32,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn unban(&self, guild: Id, user: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    fn kick(&self, guild: Id, user: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// `until = None` clears the timeout. RFC 3339, at most 28 days ahead.
    fn timeout(
        &self,
        guild: Id,
        user: Id,
        until: Option<&str>,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
}

/// The current time. Injected so tests control it.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
