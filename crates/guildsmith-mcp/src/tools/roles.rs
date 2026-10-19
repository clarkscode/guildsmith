//! `roles_list`.

use guildsmith_app::error::ServiceError;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::inspect::{self, RoleRow};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::respond;
use crate::server::GuildsmithServer;
use crate::shape::{self, Page};

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RolesParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Extra fields: color, hoist, mentionable, permissions.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Cursor from the previous page.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[tool_router(router = role_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "List a server's roles, highest first, with key fields unless more are requested.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Page<RoleRow>>()
    )]
    async fn roles_list(&self, Parameters(p): Parameters<RolesParams>) -> CallToolResult {
        let res = async {
            let s = inspect::snapshot(&self.app, p.guild.as_deref()).await?;
            let rows = inspect::roles(&s, &p.fields)?;
            shape::page(&rows, p.limit, p.cursor.as_deref()).map_err(|e| ServiceError::Invalid(e.to_string()))
        }
        .await;
        respond(res, |pg| format!("{} of {} roles", pg.items.len(), pg.total))
    }
}
