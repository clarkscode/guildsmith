//! The opt-in `webhooks` group (US5). A webhook's URL is a secret: it appears only in
//! `webhook_create`'s result, as `secrets.url`, and never in lists (FR-030).

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::{Deleted, Sent};
use guildsmith_app::services::webhooks::{self, WebhookCreated, WebhookRow, WebhookUpdated};
use guildsmith_core::asset::Asset;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::messages::MessageFields;
use super::{outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The channel it posts to.
    pub channel: String,
    /// 1-80 characters, without "clyde" or "discord".
    pub name: String,
    /// Avatar image: {url}, {data} or {file}.
    #[serde(default)]
    pub avatar: Option<Asset>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Only this channel's webhooks.
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Webhook id or name.
    pub webhook: String,
    /// New name (1-80 characters).
    #[serde(default)]
    pub name: Option<String>,
    /// New avatar: {url}, {data} or {file}.
    #[serde(default)]
    pub avatar: Option<Asset>,
    /// Move it to this channel.
    #[serde(default)]
    pub channel: Option<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Webhook id or name.
    pub webhook: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SendParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Webhook id or name.
    pub webhook: String,
    /// Post in this thread of the webhook's channel (id).
    #[serde(default)]
    pub thread: Option<String>,
    /// Show this name instead of the webhook's.
    #[serde(default)]
    pub username: Option<String>,
    /// Show this avatar image URL instead of the webhook's.
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(flatten)]
    pub message: MessageFields,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct WebhooksOutput {
    pub webhooks: Vec<WebhookRow>,
}

#[tool_router(router = webhooks_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Create a webhook in a channel; its URL is returned once, as a secret, and never again.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<WebhookCreated>()
    )]
    async fn webhook_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let res = webhooks::webhook_create(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            &p.name,
            p.avatar.as_ref(),
            p.reason.as_deref(),
        )
        .await;
        respond(res, |c| {
            format!(
                "created webhook {} ({}). secrets.url is a secret: give it only to the user, now; it isn't shown again",
                c.webhook.name, c.webhook.id
            )
        })
    }

    #[tool(
        description = "List webhooks with their channel, kind and creator; never their URLs or tokens.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<WebhooksOutput>()
    )]
    async fn webhooks_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        let res = webhooks::webhooks_list(&self.app, p.guild.as_deref(), p.channel.as_deref())
            .await
            .map(|webhooks| WebhooksOutput { webhooks });
        wrapped(res, &["name", "creator"], |o| format!("{} webhook(s)", o.webhooks.len()))
    }

    #[tool(
        description = "Rename a webhook, change its avatar or move it to another channel.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<WebhookUpdated>()
    )]
    async fn webhook_update(&self, Parameters(p): Parameters<UpdateParams>) -> CallToolResult {
        let res = webhooks::webhook_update(
            &self.app,
            p.guild.as_deref(),
            &p.webhook,
            p.name.as_deref(),
            p.avatar.as_ref(),
            p.channel.as_deref(),
            p.reason.as_deref(),
        )
        .await;
        respond(res, |u| if u.changes.is_empty() { "nothing to change".to_owned() } else { u.changes.join(", ") })
    }

    #[tool(
        description = "Delete a webhook so its URL stops working; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn webhook_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res =
            webhooks::webhook_delete(&self.app, p.guild.as_deref(), &p.webhook, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }

    #[tool(
        description = "Post a message through a webhook, optionally in a thread or under another name.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Sent>()
    )]
    async fn webhook_send(&self, Parameters(p): Parameters<SendParams>) -> CallToolResult {
        let res = webhooks::webhook_send(
            &self.app,
            p.guild.as_deref(),
            &p.webhook,
            &p.message.into_input(),
            p.thread.as_deref(),
            p.username.as_deref(),
            p.avatar_url.as_deref(),
        )
        .await;
        respond(res, |s| format!("sent {}", s.url))
    }
}
