//! The opt-in `moderation` toolset: `member_timeout`, `member_kick`, `member_ban`, `ban_remove`.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::{self, Removed, TimedOut};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{outcome_summary, respond};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TimeoutParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Member: @name, nickname or id.
    pub member: String,
    /// Seconds, at most 28 days (2419200); 0 clears the timeout.
    pub duration: u64,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct KickParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Member: @name, nickname or id.
    pub member: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BanParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Member: @name, nickname or id.
    pub member: String,
    /// Also delete their messages from this many recent seconds (max 604800).
    #[serde(default)]
    pub delete_message_seconds: u32,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnbanParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The banned user's id.
    pub user: String,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = moderation_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Time a member out (up to 28 days), or clear a timeout with duration 0.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<TimedOut>()
    )]
    async fn member_timeout(&self, Parameters(p): Parameters<TimeoutParams>) -> CallToolResult {
        let res = live::member_timeout(&self.app, p.guild.as_deref(), &p.member, p.duration, p.reason.as_deref()).await;
        respond(res, |t| match &t.until {
            Some(u) => format!("timed out until {u}"),
            None => "timeout cleared".to_owned(),
        })
    }

    #[tool(
        description = "Kick a member; without confirm=true it only returns a preview.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Removed>>()
    )]
    async fn member_kick(&self, Parameters(p): Parameters<KickParams>) -> CallToolResult {
        let res = live::member_kick(&self.app, p.guild.as_deref(), &p.member, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |r| format!("{} {}", r.action, r.member)))
    }

    #[tool(
        description = "Ban a member; without confirm=true it only returns a preview.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Removed>>()
    )]
    async fn member_ban(&self, Parameters(p): Parameters<BanParams>) -> CallToolResult {
        let res = live::member_ban(
            &self.app,
            p.guild.as_deref(),
            &p.member,
            p.delete_message_seconds,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| outcome_summary(o, |r| format!("{} {}", r.action, r.member)))
    }

    #[tool(
        description = "Lift a ban by user id.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Removed>()
    )]
    async fn ban_remove(&self, Parameters(p): Parameters<UnbanParams>) -> CallToolResult {
        let res = live::ban_remove(&self.app, p.guild.as_deref(), &p.user, p.reason.as_deref()).await;
        respond(res, |r| format!("{} {}", r.action, r.member))
    }
}
