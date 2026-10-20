//! The opt-in `moderation` group (US1): bans, unbans, the ban list, timeouts, kicks, prune,
//! message purge, bulk role assignment, the audit log and raid lockdown. Bulk tools report
//! MCP progress and stop on cancellation; member names, ban reasons and message authors are
//! wrapped as untrusted (FR-007).

use guildsmith_app::bulk::{BulkOutcome, Hooks};
use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::{DiscordPort, Incidents};
use guildsmith_app::services::moderation::{
    self, AuditRow, BansPage, BulkResult, Pruned, PurgeTarget, RoleChange, Target,
};
use guildsmith_core::Id;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{bulk_hooks, outcome_summary, requires_user_interaction, respond, wrapped};
use crate::server::GuildsmithServer;

/// Members given as a list, or one as `member`.
fn members(list: Vec<String>, one: Option<String>) -> Vec<String> {
    list.into_iter().chain(one).collect()
}

fn bulk_summary<T>(r: &BulkResult<T>) -> String {
    match r {
        BulkResult::Preview(p) => {
            let skip = if p.problems.is_empty() { String::new() } else { format!(", {} skipped", p.problems.len()) };
            format!(
                "Preview only; nothing changed. Would {} {} target(s){skip} in {} request(s). Call again with \
                 confirm: true to do it.",
                p.action,
                p.total - p.problems.len(),
                p.estimated_requests
            )
        }
        BulkResult::Done(o) => o.summary(),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BanParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Members (@name, nickname or id); user ids work for people who already left. Sent 200 per request.
    #[serde(default)]
    pub members: Vec<String>,
    /// One member, instead of members.
    #[serde(default)]
    pub member: Option<String>,
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
    /// Banned users' ids.
    #[serde(default)]
    pub users: Vec<String>,
    /// One user id, instead of users.
    #[serde(default)]
    pub user: Option<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BansParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// How many (default 100, max 1000).
    #[serde(default)]
    pub limit: Option<u16>,
    /// Only bans of users with ids below this.
    #[serde(default)]
    pub before: Option<Id>,
    /// Only bans of users with ids above this (next_after from the last page).
    #[serde(default)]
    pub after: Option<Id>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TimeoutParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Members: @name, nickname or id.
    #[serde(default)]
    pub members: Vec<String>,
    /// One member, instead of members.
    #[serde(default)]
    pub member: Option<String>,
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
    /// Members: @name, nickname or id.
    #[serde(default)]
    pub members: Vec<String>,
    /// One member, instead of members.
    #[serde(default)]
    pub member: Option<String>,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PruneParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Inactive for this many days (1-30).
    pub days: u8,
    /// Also prune members whose only roles are these.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Must be true to act; otherwise returns Discord's count.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel: #name, Category/name or id.
    pub channel: String,
    /// Only this member's messages (@name or id).
    #[serde(default)]
    pub author: Option<String>,
    /// At most this many messages (default 100, max 1000).
    #[serde(default)]
    pub count: Option<usize>,
    /// Only messages after this message id or RFC 3339 time.
    #[serde(default)]
    pub after: Option<String>,
    /// Only messages before this message id or RFC 3339 time.
    #[serde(default)]
    pub before: Option<String>,
    /// Also delete messages older than 14 days, one request each.
    #[serde(default)]
    pub include_old: bool,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BulkRolesParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Role: @Name, name or id.
    pub role: String,
    /// Members to give the role.
    #[serde(default)]
    pub add: Vec<String>,
    /// Members to take the role from.
    #[serde(default)]
    pub remove: Vec<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AuditParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Action, e.g. member_ban_add, channel_delete, or its number.
    #[serde(default)]
    pub action_type: Option<String>,
    /// Only actions by this member (@name or id).
    #[serde(default)]
    pub user: Option<String>,
    /// Only entries older than this entry id.
    #[serde(default)]
    pub before: Option<Id>,
    /// How many (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LockdownParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Pause new invites for this many seconds (max 86400).
    #[serde(default)]
    pub pause_invites_for: Option<u64>,
    /// Pause DMs between members for this many seconds (max 86400).
    #[serde(default)]
    pub pause_dms_for: Option<u64>,
    /// Turn invites and DMs back on now.
    #[serde(default)]
    pub clear: bool,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct AuditOutput {
    pub entries: Vec<AuditRow>,
}

#[tool_router(router = moderation_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Ban members or user ids (200 per request), optionally deleting recent messages; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkResult<Target>>()
    )]
    async fn member_ban(
        &self,
        Parameters(p): Parameters<BanParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "ban");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let list = members(p.members, p.member);
        let res = moderation::member_ban(
            &self.app,
            p.guild.as_deref(),
            &list,
            p.delete_message_seconds,
            p.confirm,
            p.reason.as_deref(),
            &hooks,
        )
        .await;
        finish.done(progress).await;
        wrapped(res, &["name"], bulk_summary)
    }

    #[tool(
        description = "Lift bans by user id.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkOutcome<Target>>()
    )]
    async fn ban_remove(
        &self,
        Parameters(p): Parameters<UnbanParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "unban");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let list = members(p.users, p.user);
        let res = moderation::ban_remove(&self.app, p.guild.as_deref(), &list, p.reason.as_deref(), &hooks).await;
        finish.done(progress).await;
        respond(res, BulkOutcome::summary)
    }

    #[tool(
        description = "List bans with their reasons, in pages.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<BansPage>()
    )]
    async fn bans_list(&self, Parameters(p): Parameters<BansParams>) -> CallToolResult {
        let res = moderation::bans_list(&self.app, p.guild.as_deref(), p.limit, p.before, p.after).await;
        wrapped(res, &["name", "reason"], |b| format!("{} ban(s)", b.bans.len()))
    }

    #[tool(
        description = "Time members out (up to 28 days), or clear timeouts with duration 0.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<BulkOutcome<Target>>()
    )]
    async fn member_timeout(
        &self,
        Parameters(p): Parameters<TimeoutParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "timeout");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let list = members(p.members, p.member);
        let res =
            moderation::member_timeout(&self.app, p.guild.as_deref(), &list, p.duration, p.reason.as_deref(), &hooks)
                .await;
        finish.done(progress).await;
        wrapped(res, &["name"], BulkOutcome::summary)
    }

    #[tool(
        description = "Kick members; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkResult<Target>>()
    )]
    async fn member_kick(
        &self,
        Parameters(p): Parameters<KickParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "kick");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let list = members(p.members, p.member);
        let res =
            moderation::member_kick(&self.app, p.guild.as_deref(), &list, p.confirm, p.reason.as_deref(), &hooks).await;
        finish.done(progress).await;
        wrapped(res, &["name"], bulk_summary)
    }

    #[tool(
        description = "Remove members inactive for N days (no roles, or only the given roles); previews Discord's count unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Pruned>>()
    )]
    async fn members_prune(&self, Parameters(p): Parameters<PruneParams>) -> CallToolResult {
        let res =
            moderation::members_prune(&self.app, p.guild.as_deref(), p.days, &p.roles, p.confirm, p.reason.as_deref())
                .await;
        respond(res, |o| {
            outcome_summary(o, |r| match r.pruned {
                Some(n) => format!("pruned {n} member(s)"),
                None => "prune started (Discord didn't count on a server this size)".to_owned(),
            })
        })
    }

    #[tool(
        description = "Delete recent messages in a channel, optionally one author's; previews unless confirm. Older than 14 days needs include_old.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkResult<PurgeTarget>>(),
        meta = requires_user_interaction()
    )]
    async fn messages_purge(
        &self,
        Parameters(p): Parameters<PurgeParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "purge");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let res = moderation::messages_purge(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            p.author.as_deref(),
            p.count,
            p.after.as_deref(),
            p.before.as_deref(),
            p.include_old,
            p.confirm,
            p.reason.as_deref(),
            &hooks,
        )
        .await;
        finish.done(progress).await;
        wrapped(res, &["author"], bulk_summary)
    }

    #[tool(
        description = "Give one role to many members and take it from others.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkOutcome<RoleChange>>()
    )]
    async fn roles_bulk_assign(
        &self,
        Parameters(p): Parameters<BulkRolesParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "role changes");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let res = moderation::roles_bulk_assign(
            &self.app,
            p.guild.as_deref(),
            &p.role,
            &p.add,
            &p.remove,
            p.reason.as_deref(),
            &hooks,
        )
        .await;
        finish.done(progress).await;
        wrapped(res, &["name"], BulkOutcome::summary)
    }

    #[tool(
        description = "Read the server's audit log, newest first, filtered by action or member.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<AuditOutput>()
    )]
    async fn audit_log(&self, Parameters(p): Parameters<AuditParams>) -> CallToolResult {
        let res = moderation::audit_log(
            &self.app,
            p.guild.as_deref(),
            p.action_type.as_deref(),
            p.user.as_deref(),
            p.before,
            p.limit,
        )
        .await
        .map(|entries| AuditOutput { entries });
        wrapped(res, &["reason"], |o| format!("{} entries", o.entries.len()))
    }

    #[tool(
        description = "Pause invites and/or DMs for up to 24 hours during a raid, or clear the pause; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Incidents>>()
    )]
    async fn raid_lockdown(&self, Parameters(p): Parameters<LockdownParams>) -> CallToolResult {
        let res = moderation::raid_lockdown(
            &self.app,
            p.guild.as_deref(),
            p.pause_invites_for,
            p.pause_dms_for,
            p.clear,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| {
            outcome_summary(o, |i| {
                let part = |what: &str, t: &Option<String>| match t {
                    Some(t) => format!("{what} paused until {t}"),
                    None => format!("{what} on"),
                };
                format!("{}; {}", part("invites", &i.invites_disabled_until), part("DMs", &i.dms_disabled_until))
            })
        })
    }
}
