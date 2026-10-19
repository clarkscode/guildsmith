//! `channels_tree`, `channel_upsert`, `channel_delete` and `channels_reorder`.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::inspect::{self, TreeNode};
use guildsmith_app::services::live::{self, ChannelInput, Deleted, Moved, Upserted};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{outcome_summary, requires_user_interaction, respond};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TreeParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Extra fields: position, topic, nsfw.
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct TreeOutput {
    pub tree: Vec<TreeNode>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpsertParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel to update (#name, Category/name or id). Omit to create one.
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// text, announcement, voice, stage, forum, media or category (default text).
    #[serde(default)]
    pub kind: Option<String>,
    /// Category name or id; "none" for the top level.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub nsfw: Option<bool>,
    /// Seconds between messages, 0–21600.
    #[serde(default)]
    pub slowmode: Option<u32>,
    /// Voice and stage only.
    #[serde(default)]
    pub bitrate: Option<u32>,
    /// Voice and stage only; 0 = unlimited.
    #[serde(default)]
    pub user_limit: Option<u32>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel or category: #name, Category/name or id.
    pub channel: String,
    /// Must be true to delete; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReorderParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channels in their new order, first at the top.
    pub order: Vec<String>,
    /// Move them into this category first ("none" for the top level).
    #[serde(default)]
    pub parent: Option<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = channel_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Show a server's categories and channels as a tree with names, kinds and ids.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<TreeOutput>()
    )]
    async fn channels_tree(&self, Parameters(p): Parameters<TreeParams>) -> CallToolResult {
        let res = async {
            let s = inspect::snapshot(&self.app, p.guild.as_deref()).await?;
            Ok(TreeOutput { tree: inspect::tree(&s, &p.fields)? })
        }
        .await;
        respond(res, |t| format!("{} top-level entries", t.tree.len()))
    }

    #[tool(
        description = "Create a channel or category, or update one; only differing fields are sent.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Upserted>()
    )]
    async fn channel_upsert(&self, Parameters(p): Parameters<UpsertParams>) -> CallToolResult {
        let input = ChannelInput {
            name: p.name,
            kind: p.kind,
            parent: p.parent,
            topic: p.topic,
            nsfw: p.nsfw,
            slowmode: p.slowmode,
            bitrate: p.bitrate,
            user_limit: p.user_limit,
            rtc_region: None,
        };
        let res =
            live::channel_upsert(&self.app, p.guild.as_deref(), p.channel.as_deref(), &input, p.reason.as_deref())
                .await;
        respond(res, |u| match (u.created, u.changes.is_empty()) {
            (true, _) => format!("created channel {}", u.id),
            (false, true) => "no changes needed".to_owned(),
            (false, false) => format!("updated: {}", u.changes.join(", ")),
        })
    }

    #[tool(
        description = "Delete a channel or category; without confirm=true it only returns a preview.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>(),
        meta = requires_user_interaction()
    )]
    async fn channel_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res = live::channel_delete(&self.app, p.guild.as_deref(), &p.channel, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }

    #[tool(
        description = "Put channels in the given order with one positions request.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Moved>()
    )]
    async fn channels_reorder(&self, Parameters(p): Parameters<ReorderParams>) -> CallToolResult {
        let res =
            live::channels_reorder(&self.app, p.guild.as_deref(), &p.order, p.parent.as_deref(), p.reason.as_deref())
                .await;
        respond(res, |m| format!("moved {} channel(s)", m.moved))
    }
}
