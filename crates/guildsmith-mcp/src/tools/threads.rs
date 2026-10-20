//! The opt-in `threads` group (US2): threads, forum and media posts, thread members and forum
//! tags. Thread names are user-written, so text blocks carry them wrapped as untrusted.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::Deleted;
use guildsmith_app::services::threads::{
    self, ForumTags, MemberAction, TagAction, ThreadInput, ThreadMembers, ThreadRow, ThreadState, ThreadUpdate,
    ThreadUpdated, ThreadsPage,
};
use guildsmith_core::Id;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::messages::MessageFields;
use super::{outcome_summary, quiet_outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// A text, announcement, forum or media channel.
    pub channel: String,
    /// 1-100 characters.
    pub name: String,
    /// Start the thread on this message (text channels).
    #[serde(default)]
    pub from_message: Option<Id>,
    /// A private thread (text channels).
    #[serde(default)]
    pub private: bool,
    /// Minutes of inactivity before archiving: 60, 1440, 4320 or 10080.
    #[serde(default)]
    pub auto_archive: Option<u32>,
    /// Forum/media posts: tag names (at most 5).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Forum/media posts: the first message.
    #[serde(default)]
    pub message: Option<MessageFields>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Thread id, or the exact name of an active thread.
    pub thread: String,
    /// 1-100 characters.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub locked: Option<bool>,
    /// 60, 1440, 4320 or 10080 minutes.
    #[serde(default)]
    pub auto_archive: Option<u32>,
    /// Forum posts: the full list of tag names.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Forum posts: pin to the top of the forum.
    #[serde(default)]
    pub pinned: Option<bool>,
    /// Seconds between messages per member (max 21600).
    #[serde(default)]
    pub slowmode: Option<u32>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Thread id, or the exact name of an active thread.
    pub thread: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Only this channel's threads (needed for archived ones).
    #[serde(default)]
    pub channel: Option<String>,
    /// active (default), archived_public or archived_private.
    #[serde(default)]
    pub state: ThreadState,
    /// How many archived threads (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u8>,
    /// Archived threads before this RFC 3339 archive time.
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MembersParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Thread id, or the exact name of an active thread.
    pub thread: String,
    /// list, add or remove.
    pub action: MemberAction,
    /// For add and remove: @name or id.
    #[serde(default)]
    pub member: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TagsParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// A forum or media channel.
    pub channel: String,
    /// create, rename, delete (needs confirm) or default_reaction.
    pub action: TagAction,
    /// The tag to rename or delete (name or id).
    #[serde(default)]
    pub tag: Option<String>,
    /// The new tag's name, or the new name (1-20 characters).
    #[serde(default)]
    pub name: Option<String>,
    /// The tag's emoji, or the default reaction (empty clears it).
    #[serde(default)]
    pub emoji: Option<String>,
    /// Only moderators can apply the tag.
    #[serde(default)]
    pub moderated: Option<bool>,
    /// For delete: must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = threads_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Start a thread in a text channel, or a post with tags and a first message in a forum or media channel.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<ThreadRow>()
    )]
    async fn thread_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let input = ThreadInput {
            name: p.name,
            from_message: p.from_message,
            private: p.private,
            auto_archive: p.auto_archive,
            tags: p.tags,
            message: p.message.map(MessageFields::into_input),
        };
        let res = threads::thread_create(&self.app, p.guild.as_deref(), &p.channel, &input, p.reason.as_deref()).await;
        wrapped(res, &["name"], |t| format!("created {} {}", t.kind, t.id))
    }

    #[tool(
        description = "Rename, archive, unarchive, lock, re-tag or pin a thread or forum post.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ThreadUpdated>()
    )]
    async fn thread_update(&self, Parameters(p): Parameters<UpdateParams>) -> CallToolResult {
        let input = ThreadUpdate {
            name: p.name,
            archived: p.archived,
            locked: p.locked,
            auto_archive: p.auto_archive,
            tags: p.tags,
            pinned: p.pinned,
            slowmode: p.slowmode,
        };
        let res = threads::thread_update(&self.app, p.guild.as_deref(), &p.thread, &input, p.reason.as_deref()).await;
        wrapped(res, &["name"], |u| {
            if u.changes.is_empty() { "nothing to change".to_owned() } else { format!("{} change(s)", u.changes.len()) }
        })
    }

    #[tool(
        description = "Delete a thread or forum post and its messages; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn thread_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res =
            threads::thread_delete(&self.app, p.guild.as_deref(), &p.thread, p.confirm, p.reason.as_deref()).await;
        wrapped(res, &["would"], |o| quiet_outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }

    #[tool(
        description = "List active threads, or a channel's archived public or private threads.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ThreadsPage>()
    )]
    async fn threads_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        let res = threads::threads_list(
            &self.app,
            p.guild.as_deref(),
            p.channel.as_deref(),
            p.state,
            p.limit,
            p.before.as_deref(),
        )
        .await;
        wrapped(res, &["name"], |t| format!("{} thread(s)", t.threads.len()))
    }

    #[tool(
        description = "List a thread's members, or add or remove one.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<ThreadMembers>()
    )]
    async fn thread_members(&self, Parameters(p): Parameters<MembersParams>) -> CallToolResult {
        let res =
            threads::thread_members(&self.app, p.guild.as_deref(), &p.thread, p.action, p.member.as_deref()).await;
        respond(res, |m| format!("{} member(s)", m.members.len()))
    }

    #[tool(
        description = "Create, rename or delete a forum's tags, or set its default reaction.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<ForumTags>>()
    )]
    async fn forum_tags(&self, Parameters(p): Parameters<TagsParams>) -> CallToolResult {
        let res = threads::forum_tags(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            p.action,
            p.tag.as_deref(),
            p.name.as_deref(),
            p.emoji.as_deref(),
            p.moderated,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| outcome_summary(o, |t| t.done.clone()))
    }
}
