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

use crate::policy::Toolset;

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
        }
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

/// The server permissions each agent tool needs, or `None` for an unknown tool.
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
        | "bot_preflight" => &[],
        "spec_apply" => &[P::Administrator],
        "role_upsert" | "role_delete" | "member_roles" => &[P::ManageRoles],
        "channel_upsert" | "channel_delete" | "channels_reorder" => &[P::ManageChannels],
        "permissions_set" => &[P::ManageChannels, P::ManageRoles],
        "message_send" => &[P::ViewChannel, P::SendMessages],
        "messages_read" => &[P::ViewChannel, P::ReadMessageHistory],
        "member_timeout" => &[P::ModerateMembers],
        "member_kick" => &[P::KickMembers],
        "member_ban" | "ban_remove" => &[P::BanMembers],
        _ => return None,
    })
}

/// The tools in each toolset.
#[must_use]
pub fn toolset_tools(t: Toolset) -> &'static [&'static str] {
    match t {
        Toolset::Core => &[
            "guilds_list",
            "guild_overview",
            "refresh",
            "spec_export",
            "spec_validate",
            "spec_plan",
            "spec_apply",
            "roles_list",
            "role_upsert",
            "role_delete",
            "channels_tree",
            "channel_upsert",
            "channel_delete",
            "channels_reorder",
            "permissions_set",
            "permissions_explain",
            "members_find",
            "member_roles",
            "message_send",
            "messages_read",
            "bot_preflight",
        ],
        Toolset::Moderation => &["member_timeout", "member_kick", "member_ban", "ban_remove"],
    }
}

/// Missing server permissions for `tools`, one problem per permission (naming the first tool
/// that needs it). `spec_apply`'s Administrator need is reported as `NotAdministrator`.
#[must_use]
pub fn check_tools<'a>(snapshot: &Snapshot, tools: impl IntoIterator<Item = &'a str>) -> Vec<PreflightProblem> {
    let mut c = Checks { s: snapshot, out: BTreeSet::new() };
    for tool in tools {
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
