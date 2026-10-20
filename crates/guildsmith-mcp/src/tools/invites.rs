//! The opt-in `invites` group (US3): create invites, list them with uses and expiry, revoke them,
//! and limit them to users.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::invites::{self, InviteInput, InviteRow, InviteTargets, TargetAction};
use guildsmith_app::services::live::Deleted;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel the invite opens.
    pub channel: String,
    /// Seconds until it expires (max 604800, 0 = never). Default 86400.
    #[serde(default)]
    pub max_age: Option<u32>,
    /// How many uses (max 100, 0 = unlimited).
    #[serde(default)]
    pub max_uses: Option<u32>,
    /// Members who join leave again unless given a role.
    #[serde(default)]
    pub temporary: bool,
    /// Roles new members get (below the bot's role).
    #[serde(default)]
    pub roles: Vec<String>,
    /// Only these users can use it.
    #[serde(default)]
    pub only_users: Vec<String>,
    /// Always a new code, even if a matching invite exists.
    #[serde(default)]
    pub unique: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Only this channel's invites.
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RevokeParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The invite code or link.
    pub code: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TargetsParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The invite code or link.
    pub code: String,
    /// list, add or remove.
    pub action: TargetAction,
    /// Users to add or remove (max 1000).
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct InvitesOutput {
    pub invites: Vec<InviteRow>,
}

#[tool_router(router = invites_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Create an invite to a channel with an expiry, use limit, roles or allowed users, and return its link.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<InviteRow>()
    )]
    async fn invite_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let input = InviteInput {
            max_age: p.max_age,
            max_uses: p.max_uses,
            temporary: p.temporary,
            roles: p.roles,
            only_users: p.only_users,
            unique: p.unique,
        };
        let res = invites::invite_create(&self.app, p.guild.as_deref(), &p.channel, &input, p.reason.as_deref()).await;
        respond(res, |i| format!("invite {}", i.url))
    }

    #[tool(
        description = "List invites with their uses, expiry and state (active, expired, used up).",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<InvitesOutput>()
    )]
    async fn invites_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        let res = invites::invites_list(&self.app, p.guild.as_deref(), p.channel.as_deref())
            .await
            .map(|invites| InvitesOutput { invites });
        wrapped(res, &["inviter"], |o| format!("{} invite(s)", o.invites.len()))
    }

    #[tool(
        description = "Revoke an invite so its link stops working; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn invite_revoke(&self, Parameters(p): Parameters<RevokeParams>) -> CallToolResult {
        let res = invites::invite_revoke(&self.app, p.guild.as_deref(), &p.code, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |d| format!("revoked {}", d.deleted)))
    }

    #[tool(
        description = "List, add or remove the users an invite is limited to.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<InviteTargets>()
    )]
    async fn invite_target_users(&self, Parameters(p): Parameters<TargetsParams>) -> CallToolResult {
        let res = invites::invite_target_users(&self.app, p.guild.as_deref(), &p.code, p.action, &p.users).await;
        respond(res, |t| {
            if t.users.is_empty() {
                "anyone can use it".to_owned()
            } else {
                format!("limited to {} user(s)", t.users.len())
            }
        })
    }
}
