//! `roles_list`, `role_upsert` and `role_delete`.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::inspect::{self, RoleRow};
use guildsmith_app::services::live::{self, Deleted, RoleInput, Upserted};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{outcome_summary, requires_user_interaction, respond};
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

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpsertParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Role to update (name, mention or id). Omit to create a role.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// `#RRGGBB`.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub hoist: Option<bool>,
    #[serde(default)]
    pub mentionable: Option<bool>,
    /// The full permission list, as snake_case names.
    #[serde(default)]
    pub permissions: Option<Vec<String>>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Role name, mention or id.
    pub role: String,
    /// Must be true to delete; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
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

    #[tool(
        description = "Create a role, or update one by name or id; only differing fields are sent.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Upserted>()
    )]
    async fn role_upsert(&self, Parameters(p): Parameters<UpsertParams>) -> CallToolResult {
        let input = RoleInput {
            name: p.name,
            color: p.color,
            hoist: p.hoist,
            mentionable: p.mentionable,
            permissions: p.permissions,
        };
        let res =
            live::role_upsert(&self.app, p.guild.as_deref(), p.role.as_deref(), &input, p.reason.as_deref()).await;
        respond(res, |u| match (u.created, u.changes.is_empty()) {
            (true, _) => format!("created role {}", u.id),
            (false, true) => "no changes needed".to_owned(),
            (false, false) => format!("updated: {}", u.changes.join(", ")),
        })
    }

    #[tool(
        description = "Delete a role; without confirm=true it only returns a preview.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>(),
        meta = requires_user_interaction()
    )]
    async fn role_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res = live::role_delete(&self.app, p.guild.as_deref(), &p.role, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }
}
