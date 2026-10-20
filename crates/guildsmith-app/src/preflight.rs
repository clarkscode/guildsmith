//! Permission preflight (FR-020, data-model §5, research R6/R7).
//!
//! Every problem is collected before anything is reported, so the user fixes them all at once.
//! Channel-level checks use the same explainer as `permissions_explain` (research R13).

use std::collections::BTreeSet;
use std::fmt;

use guildsmith_core::explain::{MemberInfo, Who, explain};
use guildsmith_core::planner::{Op, OpKind, OverwriteOp, Ref};
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::{Id, Permission, PermissionSet, Plan};
use schemars::JsonSchema;
use serde::Serialize;

use crate::toolsets::Toolset;

/// One thing that would make a change fail or be unsafe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreflightProblem {
    /// The bot lacks a server permission an operation needs.
    MissingPermission { perm: String, needed_by: String },
    /// A role the plan edits is at or above the bot's highest role.
    RoleTooHigh { role: String, bot_top: i64 },
    /// Spec apply needs Administrator (research R6/R7).
    NotAdministrator { reason: String },
    /// The bot can't grant or deny a permission it doesn't have itself.
    CannotGrant { perm: String, channel: String },
    /// Only a managed role's permissions can change.
    ManagedRoleEdit { role: String, field: String },
    /// A privileged gateway intent isn't turned on for the bot's application.
    MissingIntent { intent: String, needed_by: String },
    /// The server lacks a feature an operation needs (e.g. `COMMUNITY`).
    MissingFeature { feature: String, needed_by: String },
}

impl PreflightProblem {
    /// What to do about it.
    #[must_use]
    pub fn fix(&self) -> String {
        match self {
            PreflightProblem::MissingPermission { perm, .. } => {
                format!("give the bot's role the `{perm}` permission in Server Settings → Roles")
            }
            PreflightProblem::RoleTooHigh { role, .. } => {
                format!("drag the bot's role above {role} in Server Settings → Roles")
            }
            PreflightProblem::NotAdministrator { .. } => {
                "give the bot's role Administrator, or use the live tools, which need narrower permissions".to_owned()
            }
            PreflightProblem::CannotGrant { perm, .. } => {
                format!("give the bot `{perm}` (or Administrator), or remove it from the spec")
            }
            PreflightProblem::ManagedRoleEdit { field, .. } => {
                format!("remove `{field}` from the managed role in the spec; only permissions can change")
            }
            PreflightProblem::MissingIntent { intent, .. } => format!(
                "turn on the {intent} privileged intent in the Discord developer portal (your app → Bot → \
                 Privileged Gateway Intents)"
            ),
            PreflightProblem::MissingFeature { feature, .. } => {
                format!("enable {feature} for the server in Server Settings, or skip this action")
            }
        }
    }
}

impl fmt::Display for PreflightProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreflightProblem::MissingPermission { perm, needed_by } => {
                write!(f, "the bot is missing `{perm}` (needed by {needed_by})")
            }
            PreflightProblem::RoleTooHigh { role, bot_top } => {
                write!(f, "{role} is at or above the bot's highest role (position {bot_top})")
            }
            PreflightProblem::NotAdministrator { reason } => write!(f, "the bot is not an administrator: {reason}"),
            PreflightProblem::CannotGrant { perm, channel } => {
                write!(f, "the bot can't grant or deny `{perm}` on {channel} because it doesn't have it")
            }
            PreflightProblem::ManagedRoleEdit { role, field } => {
                write!(f, "{role} is a managed role; its {field} can't be changed")
            }
            PreflightProblem::MissingIntent { intent, needed_by } => {
                write!(f, "the {intent} intent is off (needed by {needed_by})")
            }
            PreflightProblem::MissingFeature { feature, needed_by } => {
                write!(f, "the server doesn't have the {feature} feature (needed by {needed_by})")
            }
        }
    }
}

const NOT_ADMIN_REASON: &str = "spec apply needs Administrator so the bot sees private channels and can set any \
                                 permission in overwrites";

fn is_owner(s: &Snapshot) -> bool {
    s.bot.user_id == s.guild.owner_id
}

fn is_admin(s: &Snapshot) -> bool {
    is_owner(s) || s.bot.permissions.contains(Permission::Administrator)
}

fn role_label(s: &Snapshot, id: Id) -> String {
    s.role(id).map_or_else(|| format!("role {id}"), |r| format!("@{}", r.name))
}

/// The bot's permissions in one channel, through the explainer.
fn bot_in_channel(s: &Snapshot, channel: Id) -> PermissionSet {
    let me = MemberInfo { id: s.bot.user_id, role_ids: s.bot.role_ids.clone(), timed_out: false };
    explain(s, channel, &Who::Member(me), None).map_or(s.bot.permissions, |e| e.allowed)
}

struct Checks<'a> {
    s: &'a Snapshot,
    out: BTreeSet<PreflightProblem>,
}

impl Checks<'_> {
    fn need(&mut self, perm: Permission, needed_by: &str) {
        if !is_admin(self.s) && !self.s.bot.permissions.contains(perm) {
            // One entry per permission: the first op that needs it.
            if !self
                .out
                .iter()
                .any(|p| matches!(p, PreflightProblem::MissingPermission { perm: q, .. } if q == perm.name()))
            {
                self.out.insert(PreflightProblem::MissingPermission {
                    perm: perm.name().to_owned(),
                    needed_by: needed_by.to_owned(),
                });
            }
        }
    }

    fn role_below_bot(&mut self, id: Id, position: Option<i64>) {
        let s = self.s;
        if is_owner(s) || id == s.guild.id {
            return;
        }
        let at = position.or_else(|| s.role(id).map(|r| r.position));
        if at.is_some_and(|p| p >= s.bot.top_role_position) {
            self.out
                .insert(PreflightProblem::RoleTooHigh { role: role_label(s, id), bot_top: s.bot.top_role_position });
        }
    }

    fn grants(&mut self, perms: PermissionSet, have: PermissionSet, place: &str) {
        if is_admin(self.s) {
            return;
        }
        for p in perms.iter() {
            // Only an administrator can put manage_roles in an overwrite (research R7).
            if !have.contains(p) || p == Permission::ManageRoles {
                self.out.insert(PreflightProblem::CannotGrant { perm: p.name().to_owned(), channel: place.to_owned() });
            }
        }
    }

    fn overwrites(&mut self, list: &[OverwriteOp], channel: Option<Id>, place: &str) {
        let have = channel.map_or(self.s.bot.permissions, |c| bot_in_channel(self.s, c));
        for o in list {
            self.grants(o.allow.union(o.deny), have, place);
        }
    }

    fn op(&mut self, op: &Op) {
        let s = self.s;
        match &op.kind {
            OpKind::CreateRole { attrs, key } => {
                self.need(Permission::ManageRoles, &op.summary);
                if let Some(p) = attrs.permissions {
                    self.grants(p, s.bot.permissions, &format!("role @{}", key.trim_start_matches("role:")));
                }
            }
            OpKind::UpdateRole { id, attrs, changes } => {
                self.need(Permission::ManageRoles, &op.summary);
                self.role_below_bot(*id, None);
                if let Some(p) = attrs.permissions {
                    let current = s.role(*id).map_or(PermissionSet::EMPTY, |r| r.permissions);
                    // Only permissions being added need to be held.
                    self.grants(p.difference(current), s.bot.permissions, &format!("role {}", role_label(s, *id)));
                }
                if s.role(*id).is_some_and(|r| r.managed) {
                    for c in changes.iter().filter(|c| c.field != "permissions") {
                        self.out.insert(PreflightProblem::ManagedRoleEdit {
                            role: role_label(s, *id),
                            field: c.field.clone(),
                        });
                    }
                }
            }
            OpKind::ReorderRoles { order } => {
                self.need(Permission::ManageRoles, &op.summary);
                for slot in order {
                    if let Ref::Id(id) = slot.role {
                        self.role_below_bot(id, None);
                        if let Some(p) = slot.position {
                            self.role_below_bot(id, Some(p));
                        }
                    }
                }
            }
            OpKind::CreateChannel { overwrites, attrs, .. } => {
                self.need(Permission::ManageChannels, &op.summary);
                if let Some(list) = overwrites.as_deref().filter(|l| !l.is_empty()) {
                    self.need(Permission::ManageRoles, &op.summary);
                    let what = if attrs.kind == Some(LiveChannelKind::Category) { "category" } else { "channel" };
                    let name = attrs.name.as_deref().unwrap_or_default();
                    self.overwrites(list, None, &format!("new {what} {name}"));
                }
            }
            OpKind::UpdateChannel { id, overwrites, .. } => {
                self.need(Permission::ManageChannels, &op.summary);
                if let Some(list) = overwrites {
                    self.need(Permission::ManageRoles, &op.summary);
                    let name = s.channel(*id).map_or_else(|| id.to_string(), |c| c.name.clone());
                    self.overwrites(list, Some(*id), &name);
                }
            }
            OpKind::ReorderChannels { .. } | OpKind::DeleteChannel { .. } => {
                self.need(Permission::ManageChannels, &op.summary);
            }
            OpKind::DeleteRole { id } => {
                self.need(Permission::ManageRoles, &op.summary);
                self.role_below_bot(*id, None);
            }
            other => {
                for p in kind_needs(other) {
                    self.need(*p, &op.summary);
                }
            }
        }
    }
}

/// The server permissions an op of the kinds beyond roles and channels needs.
fn kind_needs(kind: &OpKind) -> &'static [Permission] {
    use Permission as P;
    match kind {
        OpKind::UpdateSettings { .. }
        | OpKind::CreateAutomod { .. }
        | OpKind::UpdateAutomod { .. }
        | OpKind::DeleteAutomod { .. }
        | OpKind::SetWelcomeScreen { .. }
        | OpKind::DeleteInvite { .. } => &[P::ManageGuild],
        OpKind::SetOnboarding { .. } => &[P::ManageGuild, P::ManageRoles],
        OpKind::CreateExpression { .. } => &[P::CreateGuildExpressions],
        OpKind::UpdateExpression { .. } | OpKind::DeleteExpression { .. } => &[P::ManageGuildExpressions],
        OpKind::CreateWebhook { .. } | OpKind::UpdateWebhook { .. } | OpKind::DeleteWebhook { .. } => {
            &[P::ManageWebhooks]
        }
        OpKind::CreateInvite { .. } => &[P::CreateInstantInvite],
        OpKind::CreateEvent { .. } => &[P::CreateEvents],
        OpKind::UpdateEvent { .. } | OpKind::DeleteEvent { .. } => &[P::ManageEvents],
        OpKind::PostMessage { pin: true, .. } => &[P::SendMessages, P::PinMessages],
        OpKind::PostMessage { .. } => &[P::SendMessages],
        OpKind::SetPinned { .. } => &[P::PinMessages],
        // The bot's own messages: no permission needed to edit or delete them.
        _ => &[],
    }
}

/// Everything that stops `plan` from applying cleanly on `snapshot`. Empty means go.
#[must_use]
pub fn check_plan(snapshot: &Snapshot, plan: &Plan) -> Vec<PreflightProblem> {
    let mut c = Checks { s: snapshot, out: BTreeSet::new() };
    if !plan.ops.is_empty() && !is_admin(snapshot) {
        c.out.insert(PreflightProblem::NotAdministrator { reason: NOT_ADMIN_REASON.to_owned() });
    }
    for op in &plan.ops {
        c.op(op);
    }
    c.out.into_iter().collect()
}

/// The server permissions each agent tool needs, or `None` for an unknown tool. Needs that
/// depend on the options used (e.g. bulk ban's `manage_guild`) are checked by the service.
#[must_use]
pub fn tool_needs(tool: &str) -> Option<&'static [Permission]> {
    use Permission as P;
    Some(match tool {
        "guilds_list"
        | "guild_overview"
        | "refresh"
        | "spec_export"
        | "spec_validate"
        | "spec_plan"
        | "roles_list"
        | "channels_tree"
        | "permissions_explain"
        | "members_find"
        | "bot_preflight"
        | "toolsets"
        | "tool_call"
        | "events_list"
        | "event_users"
        | "voice_state"
        | "expressions_list"
        | "template_import"
        | "webhook_send"
        | "discord_request" => &[],
        "spec_apply" => &[P::Administrator],
        "role_upsert" | "role_delete" | "member_roles" | "roles_bulk_assign" => &[P::ManageRoles],
        "channel_upsert" | "channel_delete" | "channels_reorder" | "forum_tags" | "invite_revoke" => {
            &[P::ManageChannels]
        }
        "permissions_set" => &[P::ManageChannels, P::ManageRoles],
        "message_send" | "message_compose" => &[P::ViewChannel, P::SendMessages],
        "messages_read" | "pins_list" | "reactions_list" | "messages_search" | "threads_list" | "poll_results" => {
            &[P::ViewChannel, P::ReadMessageHistory]
        }
        "message_edit" | "message_crosspost" | "poll_end" => &[P::ViewChannel],
        "message_delete" => &[P::ViewChannel, P::ManageMessages],
        "message_pin" => &[P::ViewChannel, P::PinMessages],
        "reactions" => &[P::ViewChannel, P::ReadMessageHistory, P::AddReactions],
        "member_timeout" => &[P::ModerateMembers],
        "member_kick" | "members_prune" => &[P::KickMembers],
        "member_ban" | "ban_remove" | "bans_list" => &[P::BanMembers],
        "messages_purge" => &[P::ViewChannel, P::ReadMessageHistory, P::ManageMessages],
        "audit_log" => &[P::ViewAuditLog],
        "raid_lockdown"
        | "invites_list"
        | "invite_target_users"
        | "automod_rules_list"
        | "automod_rule_upsert"
        | "automod_rule_delete"
        | "settings_get"
        | "settings_update"
        | "welcome_screen_update"
        | "templates"
        | "integration_remove" => &[P::ManageGuild],
        "thread_create" => &[P::ViewChannel, P::CreatePublicThreads],
        "thread_update" | "thread_delete" | "thread_members" => &[P::ManageThreads],
        "poll_create" => &[P::ViewChannel, P::SendMessages, P::SendPolls],
        "event_create" => &[P::CreateEvents],
        "event_update" | "event_delete" => &[P::ManageEvents],
        "invite_create" => &[P::CreateInstantInvite],
        "voice_members" => &[P::MoveMembers],
        "stage" => &[P::ManageChannels, P::MuteMembers, P::MoveMembers],
        "voice_channel_status" => &[P::SetVoiceChannelStatus],
        "webhook_create" | "webhooks_list" | "webhook_update" | "webhook_delete" => &[P::ManageWebhooks],
        "expression_create" => &[P::CreateGuildExpressions],
        "expression_update" | "expression_delete" => &[P::ManageGuildExpressions],
        "onboarding_update" => &[P::ManageGuild, P::ManageRoles],
        _ => return None,
    })
}

/// The server feature that makes pruning need Administrator (research E4).
pub const PRUNE_REQUIRES_ADMIN: &str = "PRUNE_REQUIRES_ADMIN";

/// Missing server permissions for `tools`, one problem per permission (naming the first tool
/// that needs it). `spec_apply`'s Administrator need is reported as `NotAdministrator`.
#[must_use]
pub fn check_tools<'a>(snapshot: &Snapshot, tools: impl IntoIterator<Item = &'a str>) -> Vec<PreflightProblem> {
    let mut c = Checks { s: snapshot, out: BTreeSet::new() };
    for tool in tools {
        if tool == "members_prune" && snapshot.guild.features.contains(PRUNE_REQUIRES_ADMIN) {
            c.need(Permission::Administrator, "members_prune (this server requires Administrator to prune)");
        }
        for perm in tool_needs(tool).unwrap_or_default() {
            if *perm == Permission::Administrator {
                if !is_admin(snapshot) {
                    c.out.insert(PreflightProblem::NotAdministrator { reason: NOT_ADMIN_REASON.to_owned() });
                }
            } else {
                c.need(*perm, tool);
            }
        }
    }
    c.out.into_iter().collect()
}

/// Application flags that mean the `MESSAGE_CONTENT` intent is on (`GATEWAY_MESSAGE_CONTENT` and
/// `GATEWAY_MESSAGE_CONTENT_LIMITED`).
pub const MESSAGE_CONTENT_FLAGS: u64 = (1 << 18) | (1 << 19);

/// Intents the given toolsets need that the application flags don't show as on. `flags` is
/// `None` when they couldn't be read; then nothing is reported.
#[must_use]
pub fn check_intents(flags: Option<u64>, toolsets: impl IntoIterator<Item = Toolset>) -> Vec<PreflightProblem> {
    let Some(flags) = flags else { return Vec::new() };
    let mut out = BTreeSet::new();
    for t in toolsets {
        for intent in t.required_intents() {
            let on = match *intent {
                "MESSAGE_CONTENT" => flags & MESSAGE_CONTENT_FLAGS != 0,
                _ => true,
            };
            if !on {
                out.insert(PreflightProblem::MissingIntent {
                    intent: (*intent).to_owned(),
                    needed_by: format!("the {t} toolset"),
                });
            }
        }
    }
    out.into_iter().collect()
}

/// Role-order problems: managed spec roles at or above the bot (used by `doctor`).
#[must_use]
pub fn check_role_order(snapshot: &Snapshot) -> Vec<PreflightProblem> {
    let mut c = Checks { s: snapshot, out: BTreeSet::new() };
    for r in snapshot
        .roles
        .iter()
        .filter(|r| r.id != snapshot.guild.id && !r.managed && !snapshot.bot.role_ids.contains(&r.id))
    {
        c.role_below_bot(r.id, None);
    }
    c.out.into_iter().collect()
}

/// Role-order problems for a live edit of one role (or assigning it to a member).
#[must_use]
pub fn check_role_edit(snapshot: &Snapshot, role: Id) -> Vec<PreflightProblem> {
    let mut c = Checks { s: snapshot, out: BTreeSet::new() };
    c.role_below_bot(role, None);
    c.out.into_iter().collect()
}
