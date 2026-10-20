//! `ThreadsPort`: threads and forum/media posts (US2). Deleting a thread is
//! `DiscordPort::delete_channel`; forum tags are channel fields (`ChannelFields::tags`).

use std::future::Future;

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;

use super::PortResult;
use super::messages::OutgoingMessage;

/// A thread or forum/media post. `name` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThreadInfo {
    pub id: Id,
    pub parent_id: Id,
    pub name: String,
    /// `public`, `private` or `announcement`.
    pub kind: String,
    pub archived: bool,
    pub locked: bool,
    /// Minutes: 60, 1440, 4320 or 10080.
    pub auto_archive: u32,
    /// Applied forum tag ids.
    pub tags: Vec<Id>,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Id>,
    pub message_count: u32,
    pub member_count: u32,
}

/// A thread or forum post to create.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewThread {
    pub name: String,
    pub auto_archive: Option<u32>,
    pub private: bool,
    /// Start the thread on this message.
    pub from_message: Option<Id>,
    /// The first message of a forum or media post.
    pub post: Option<OutgoingMessage>,
    pub tags: Vec<Id>,
}

/// Changes to a thread. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadEdit {
    pub name: Option<String>,
    pub archived: Option<bool>,
    pub locked: Option<bool>,
    pub auto_archive: Option<u32>,
    pub tags: Option<Vec<Id>>,
    pub pinned: Option<bool>,
    pub slowmode: Option<u32>,
}

pub trait ThreadsPort: Send + Sync {
    fn thread(&self, thread: Id) -> impl Future<Output = PortResult<ThreadInfo>> + Send;
    /// A channel's flags (forum `REQUIRE_TAG` is `1 << 4`).
    fn channel_flags(&self, channel: Id) -> impl Future<Output = PortResult<u64>> + Send;
    fn create_thread(
        &self,
        channel: Id,
        thread: &NewThread,
        reason: &str,
    ) -> impl Future<Output = PortResult<ThreadInfo>> + Send;
    fn update_thread(
        &self,
        thread: Id,
        edit: &ThreadEdit,
        reason: &str,
    ) -> impl Future<Output = PortResult<ThreadInfo>> + Send;
    /// Active threads in the server.
    fn active_threads(&self, guild: Id) -> impl Future<Output = PortResult<Vec<ThreadInfo>>> + Send;
    /// Archived threads in a channel, newest first: `before` is an archive time (RFC 3339).
    /// Returns whether there are more.
    fn archived_threads(
        &self,
        channel: Id,
        private: bool,
        before: Option<&str>,
        limit: u8,
    ) -> impl Future<Output = PortResult<(Vec<ThreadInfo>, bool)>> + Send;
    fn thread_members(&self, thread: Id) -> impl Future<Output = PortResult<Vec<Id>>> + Send;
    fn add_thread_member(&self, thread: Id, user: Id) -> impl Future<Output = PortResult<()>> + Send;
    fn remove_thread_member(&self, thread: Id, user: Id) -> impl Future<Output = PortResult<()>> + Send;
}
