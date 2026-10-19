//! `permissions_set`, `permissions_explain` and `bot_preflight`.

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::doctor::{self, PreflightReport};
use guildsmith_app::services::live::{self, Explained, OverwriteSet};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::respond;
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SetParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel or category: #name, Category/name or id.
    pub channel: String,
    /// @everyone, a role, or a member (member:<id>, @name).
    pub target: String,
    /// Permissions to allow, as snake_case names.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Permissions to deny, as snake_case names.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Remove the target's overwrite instead.
    #[serde(default)]
    pub clear: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExplainParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel: #name, Category/name or id.
    pub channel: String,
    /// A member (@name, id) or a role (@Role).
    pub who: String,
    /// One permission to explain, e.g. view_channel.
    #[serde(default)]
    pub permission: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PreflightParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// spec_apply (with toml) or a tool name; omit to check every enabled tool.
    #[serde(default)]
    pub action: Option<String>,
    /// The spec text, for action spec_apply.
    #[serde(default)]
    pub toml: Option<String>,
}

#[tool_router(router = perm_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Set, replace or clear one permission overwrite on a channel or category.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<OverwriteSet>()
    )]
    async fn permissions_set(&self, Parameters(p): Parameters<SetParams>) -> CallToolResult {
        let res = live::permissions_set(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            &p.target,
            &p.allow,
            &p.deny,
            p.clear,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| match &o.overwrite {
            Some(ow) => format!("overwrite for {} set", ow.target),
            None => "overwrite cleared".to_owned(),
        })
    }

    #[tool(
        description = "Explain step by step why a member or role can or can't do things in a channel.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Explained>()
    )]
    async fn permissions_explain(&self, Parameters(p): Parameters<ExplainParams>) -> CallToolResult {
        let res =
            live::permissions_explain(&self.app, p.guild.as_deref(), &p.channel, &p.who, p.permission.as_deref()).await;
        respond(res, |e| {
            e.verdict.clone().unwrap_or_else(|| format!("{} permission(s) allowed", e.allowed.names().len()))
        })
    }

    #[tool(
        description = "Check the bot's permissions and role order for an action, listing every problem with its fix.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<PreflightReport>()
    )]
    async fn bot_preflight(&self, Parameters(p): Parameters<PreflightParams>) -> CallToolResult {
        let res = doctor::bot_preflight(&self.app, p.guild.as_deref(), p.action.as_deref(), p.toml.as_deref()).await;
        respond(res, |r| if r.ok { "ok: no problems".to_owned() } else { format!("{} problem(s)", r.problems.len()) })
    }
}
