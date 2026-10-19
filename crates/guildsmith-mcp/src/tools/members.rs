//! `members_find` and `member_roles`.

use guildsmith_app::error::ServiceError;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::{self, MemberRoles, MemberRow};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::respond;
use crate::server::GuildsmithServer;
use crate::shape::{self, Page};

/// How many matches one search fetches before paging.
const SEARCH_LIMIT: u16 = 1000;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FindParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Start of a username or nickname.
    pub query: String,
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Cursor from the previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RolesParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Member: @name, nickname or id.
    pub member: String,
    /// Roles to give.
    #[serde(default)]
    pub add: Vec<String>,
    /// Roles to take away.
    #[serde(default)]
    pub remove: Vec<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = member_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Find members whose username or nickname starts with the query.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Page<MemberRow>>()
    )]
    async fn members_find(&self, Parameters(p): Parameters<FindParams>) -> CallToolResult {
        let res = async {
            let rows = live::members_find(&self.app, p.guild.as_deref(), &p.query, SEARCH_LIMIT).await?;
            shape::page(&rows, p.limit, p.cursor.as_deref()).map_err(|e| ServiceError::Invalid(e.to_string()))
        }
        .await;
        respond(res, |pg| format!("{} of {} members", pg.items.len(), pg.total))
    }

    #[tool(
        description = "Give roles to a member and take roles away.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<MemberRoles>()
    )]
    async fn member_roles(&self, Parameters(p): Parameters<RolesParams>) -> CallToolResult {
        let res =
            live::member_roles(&self.app, p.guild.as_deref(), &p.member, &p.add, &p.remove, p.reason.as_deref()).await;
        respond(res, |m| {
            format!("member now has: {}", if m.roles.is_empty() { "no roles".to_owned() } else { m.roles.join(", ") })
        })
    }
}
