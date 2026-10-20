//! `MessagesPort`: what the `messages` group needs (US2, research E4/E11), and the message
//! types the `threads` and `polls` groups share. Deleting a message is
//! `ModerationPort::delete_message`.

use std::future::Future;

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::PortResult;

/// A message as the content tools see it. `author_name`, `content`, `embeds` and poll text are
/// user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageInfo {
    pub id: Id,
    pub channel_id: Id,
    pub author_id: Id,
    pub author_name: String,
    pub author_bot: bool,
    pub content: String,
    /// RFC 3339.
    pub timestamp: String,
    pub edited: bool,
    pub pinned: bool,
    /// Discord message flags (e.g. `1 << 2` suppress embeds, `1 << 15` Components V2).
    pub flags: u64,
    pub embeds: Vec<Value>,
    /// Components V2 layout, if the message has one.
    pub components: Option<Value>,
    pub attachments: Vec<AttachmentInfo>,
    pub reactions: Vec<ReactionInfo>,
    /// The message this one replies to or forwards.
    pub reference: Option<Id>,
    pub poll: Option<PollInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ReactionInfo {
    /// A unicode emoji, or `name:id` for a custom one.
    pub emoji: String,
    pub count: u32,
    /// Whether the bot reacted.
    pub me: bool,
}

/// A poll and its per-answer counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PollInfo {
    pub question: String,
    pub answers: Vec<PollAnswer>,
    pub multiselect: bool,
    /// RFC 3339; `null` for polls without an end.
    pub expiry: Option<String>,
    /// Whether the counts are final.
    pub finalized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PollAnswer {
    pub id: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub votes: u32,
}

/// Who may be pinged. Everything is off unless listed (FR-019).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowedMentions {
    pub users: Vec<Id>,
    pub roles: Vec<Id>,
    pub everyone: bool,
    /// Ping the author of the message replied to.
    pub replied_user: bool,
}

/// A file to attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileData {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A poll to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPoll {
    pub question: String,
    /// Text and optional emoji per answer.
    pub answers: Vec<(String, Option<String>)>,
    pub duration_hours: u32,
    pub multiselect: bool,
}

/// A message to post. Validation happens in the service.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutgoingMessage {
    pub content: Option<String>,
    pub embeds: Vec<Value>,
    pub allowed_mentions: AllowedMentions,
    pub reply_to: Option<Id>,
    /// Forward another message: (channel, message).
    pub forward: Option<(Id, Id)>,
    /// Components V2 layout (sets the `IS_COMPONENTS_V2` flag).
    pub components: Option<Value>,
    pub flags: u64,
    pub poll: Option<NewPoll>,
    pub files: Vec<FileData>,
}

/// Changes to a message. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageEdit {
    pub content: Option<String>,
    pub embeds: Option<Vec<Value>>,
    pub flags: Option<u64>,
    /// A new Components V2 layout.
    pub components: Option<Value>,
}

/// A user, from reactions or poll voters. `name` is user-chosen: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UserInfo {
    pub id: Id,
    pub name: String,
}

/// A guild message search (research E11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub content: Option<String>,
    pub authors: Vec<Id>,
    pub channels: Vec<Id>,
    /// `link`, `embed`, `file`, `image`, `video`, `sound`, `sticker`, `poll`, `snapshot`.
    pub has: Vec<String>,
    /// Only messages after / before these ids.
    pub min_id: Option<Id>,
    pub max_id: Option<Id>,
    /// ≤ 25.
    pub limit: u8,
    /// ≤ 9975.
    pub offset: u32,
}

/// What a search returned.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchOutcome {
    /// The server isn't indexed yet; try again after this many seconds.
    NotIndexed {
        retry_after: u64,
    },
    Found {
        total: u64,
        messages: Vec<MessageInfo>,
    },
}

pub trait MessagesPort: Send + Sync {
    fn message(&self, channel: Id, message: Id) -> impl Future<Output = PortResult<MessageInfo>> + Send;
    /// Recent messages, newest first (`limit` ≤ 100).
    fn recent(
        &self,
        channel: Id,
        limit: u8,
        before: Option<Id>,
    ) -> impl Future<Output = PortResult<Vec<MessageInfo>>> + Send;
    fn post(&self, channel: Id, msg: &OutgoingMessage) -> impl Future<Output = PortResult<MessageInfo>> + Send;
    fn edit(
        &self,
        channel: Id,
        message: Id,
        edit: &MessageEdit,
    ) -> impl Future<Output = PortResult<MessageInfo>> + Send;
    /// Pinned messages, newest pin first: `before` is a pin time (RFC 3339), `limit` ≤ 50.
    /// Returns whether there are more.
    fn pins(
        &self,
        channel: Id,
        before: Option<&str>,
        limit: u8,
    ) -> impl Future<Output = PortResult<(Vec<(MessageInfo, String)>, bool)>> + Send;
    fn set_pinned(
        &self,
        channel: Id,
        message: Id,
        pinned: bool,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    /// Publishes a message in an announcement channel to its followers.
    fn crosspost(&self, channel: Id, message: Id) -> impl Future<Output = PortResult<()>> + Send;
    fn react(&self, channel: Id, message: Id, emoji: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// Removes the bot's reaction, or `user`'s.
    fn unreact(
        &self,
        channel: Id,
        message: Id,
        emoji: &str,
        user: Option<Id>,
    ) -> impl Future<Output = PortResult<()>> + Send;
    /// Removes every reaction, or every reaction with `emoji`.
    fn clear_reactions(
        &self,
        channel: Id,
        message: Id,
        emoji: Option<&str>,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn reaction_users(
        &self,
        channel: Id,
        message: Id,
        emoji: &str,
        after: Option<Id>,
        limit: u8,
    ) -> impl Future<Output = PortResult<Vec<UserInfo>>> + Send;
    fn search(&self, guild: Id, query: &SearchQuery) -> impl Future<Output = PortResult<SearchOutcome>> + Send;
}
