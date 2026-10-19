//! `ApplyService`: runs a reviewed plan (FR-012, FR-013, FR-014; data-model §4 lifecycle).
//!
//! 1. Refuse in read-only mode or outside the allowlist, before any request.
//! 2. Take a **fresh** snapshot (never the cache) and rebuild the plan.
//! 3. Refuse unless the rebuilt plan's hash equals the reviewed one: the server changed, or
//!    the hash belongs to another plan.
//! 4. Refuse if preflight finds any problem, listing all of them.
//! 5. Run the ops in order, reporting progress; stop at the first error, never retry.

use std::collections::BTreeMap;

use guildsmith_core::hash::is_plan_hash;
use guildsmith_core::planner::positions::{channel_positions, role_positions};
use guildsmith_core::planner::{ChannelAttrs, Op, OpKind, OverwriteOp, Ref, RoleAttrs, ScopeOrder};
use guildsmith_core::snapshot::LiveOverwrite;
use guildsmith_core::{Id, Plan};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::policy::{Refusal, RefusalKind, audit_reason};
use crate::ports::{ChannelFields, ChannelPosition, DiscordPort, PortError, RoleFields, RolePosition};
use crate::preflight;
use crate::services::{guilds, plan};

/// The op that stopped an apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FailedOp {
    pub summary: String,
    pub error: String,
}

/// What happened during an apply.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, JsonSchema)]
pub struct ApplyReport {
    /// Ops that succeeded, in order.
    pub applied: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<FailedOp>,
    /// Ops not attempted because an earlier one failed.
    pub skipped: Vec<String>,
    /// Ids of created items, by plan key.
    #[serde(skip)]
    #[schemars(skip)]
    pub created: BTreeMap<String, Id>,
}

impl ApplyReport {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.failed.is_none()
    }
}

/// Sent before each op runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    /// 1-based.
    pub index: usize,
    pub total: usize,
    pub summary: String,
}

/// Refusal for a plan hash that doesn't match the fresh plan.
fn mismatch(given: &str) -> Refusal {
    if is_plan_hash(given) {
        Refusal::new(
            RefusalKind::PlanMismatch,
            "server changed since plan, or this plan hash belongs to a different plan; nothing was changed",
            Some("run plan again, review it, and apply with the new plan hash"),
        )
    } else {
        Refusal::new(
            RefusalKind::PlanMismatch,
            format!("`{given}` is not a plan hash from guildsmith; nothing was changed"),
            Some("run plan and pass the plan_hash it prints (gsp_…)"),
        )
    }
}

/// Refusal listing every preflight problem (FR-020).
#[must_use]
pub fn preflight_refusal(problems: &[preflight::PreflightProblem]) -> Refusal {
    let lines: Vec<String> = problems.iter().map(|p| format!("{p} → {}", p.fix())).collect();
    Refusal::new(
        RefusalKind::Preflight,
        format!("preflight found {} problem(s); nothing was changed:\n- {}", problems.len(), lines.join("\n- ")),
        Some("fix every problem listed, then plan and apply again (`guildsmith doctor` checks the bot's setup)"),
    )
    .with_details(serde_json::to_value(problems).unwrap_or_default())
}

/// Applies the plan for `text` on `guild`, if its hash is `plan_hash`.
pub async fn apply<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    text: &str,
    plan_hash: &str,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> Result<ApplyReport, ServiceError> {
    app.policy.ensure_writable()?;
    let spec = plan::parse_spec(text)?;
    let id = guilds::resolve(app, guild).await?;
    let snapshot = app.fresh_snapshot(id).await?;
    let fresh = plan::build(&spec, &snapshot)?;
    if fresh.hash != plan_hash.trim() {
        return Err(mismatch(plan_hash.trim()).into());
    }
    let problems = preflight::check_plan(&snapshot, &fresh);
    if !problems.is_empty() {
        return Err(preflight_refusal(&problems).into());
    }
    let report = run(app, id, &fresh, progress).await;
    app.invalidate(id);
    Ok(report)
}

/// Runs every op in order, stopping at the first error.
async fn run<P: DiscordPort>(
    app: &App<P>,
    guild: Id,
    plan: &Plan,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> ApplyReport {
    let mut report = ApplyReport::default();
    let total = plan.ops.len();
    for (i, op) in plan.ops.iter().enumerate() {
        progress(&Progress { index: i + 1, total, summary: op.summary.clone() });
        let mut exec = Exec { app, guild, created: &mut report.created };
        match exec.op(op).await {
            Ok(()) => report.applied.push(op.summary.clone()),
            Err(e) => {
                tracing::warn!(op = %op.summary, error = %e, "apply stopped");
                report.failed = Some(FailedOp { summary: op.summary.clone(), error: e.to_string() });
                report.skipped = plan.ops[i + 1..].iter().map(|o| o.summary.clone()).collect();
                break;
            }
        }
    }
    report
}

struct Exec<'a, P> {
    app: &'a App<P>,
    guild: Id,
    created: &'a mut BTreeMap<String, Id>,
}

impl<P: DiscordPort> Exec<'_, P> {
    fn resolve(&self, r: &Ref) -> Result<Id, PortError> {
        match r {
            Ref::Id(id) => Ok(*id),
            Ref::New(key) => {
                self.created.get(key).copied().ok_or_else(|| PortError::NotFound(format!("{key} was not created")))
            }
        }
    }

    fn overwrites(&self, list: &[OverwriteOp]) -> Result<Vec<LiveOverwrite>, PortError> {
        list.iter()
            .map(|o| Ok(LiveOverwrite { id: self.resolve(&o.target)?, kind: o.kind, allow: o.allow, deny: o.deny }))
            .collect()
    }

    /// Fields to send; the caller sets `parent_id`.
    fn channel_fields(&self, a: &ChannelAttrs, overwrites: Option<&[OverwriteOp]>) -> Result<ChannelFields, PortError> {
        Ok(ChannelFields {
            name: a.name.clone(),
            kind: a.kind,
            parent_id: None,
            position: None,
            topic: a.topic.clone(),
            nsfw: a.nsfw,
            slowmode: a.slowmode,
            bitrate: a.bitrate,
            user_limit: a.user_limit,
            rtc_region: a.rtc_region.clone(),
            tags: a.tags.clone(),
            default_reaction: a.default_reaction.clone(),
            overwrites: overwrites.map(|o| self.overwrites(o)).transpose()?,
        })
    }

    async fn reorder_channels(&self, scopes: &[ScopeOrder], reason: &str) -> Result<(), PortError> {
        // A fresh view: creates and moves earlier in this plan changed the positions.
        let snapshot = self.app.port.snapshot(self.guild).await?;
        let mut positions = Vec::new();
        for scope in scopes {
            let order = scope
                .order
                .iter()
                .map(|s| Ok((self.resolve(&s.channel)?, s.position)))
                .collect::<Result<Vec<_>, PortError>>()?;
            positions.extend(channel_positions(&snapshot, &order).into_iter().map(|(id, position)| ChannelPosition {
                id,
                position,
                parent_id: None,
            }));
        }
        if positions.is_empty() {
            return Ok(());
        }
        self.app.port.reorder_channels(self.guild, &positions, reason).await
    }

    async fn op(&mut self, op: &Op) -> Result<(), PortError> {
        let (port, guild) = (&self.app.port, self.guild);
        let reason = audit_reason(&op.summary, None);
        match &op.kind {
            OpKind::CreateRole { key, attrs } => {
                let id = port.create_role(guild, &role_fields(attrs), &reason).await?;
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateRole { id, attrs, .. } => port.update_role(guild, *id, &role_fields(attrs), &reason).await?,
            OpKind::ReorderRoles { order } => {
                let order = order
                    .iter()
                    .map(|s| Ok((self.resolve(&s.role)?, s.position)))
                    .collect::<Result<Vec<_>, PortError>>()?;
                let snapshot = port.snapshot(guild).await?;
                let positions: Vec<RolePosition> = role_positions(&snapshot, &order)
                    .into_iter()
                    .map(|(id, position)| RolePosition { id, position })
                    .collect();
                if !positions.is_empty() {
                    port.reorder_roles(guild, &positions, &reason).await?;
                }
            }
            OpKind::CreateChannel { key, parent, attrs, overwrites } => {
                let mut fields = self.channel_fields(attrs, overwrites.as_deref())?;
                fields.parent_id = Some(parent.as_ref().map(|p| self.resolve(p)).transpose()?);
                let id = port.create_channel(guild, &fields, &reason).await?;
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateChannel { id, attrs, parent, overwrites, .. } => {
                let mut fields = self.channel_fields(attrs, overwrites.as_deref())?;
                if let Some(p) = parent {
                    fields.parent_id = Some(p.as_ref().map(|r| self.resolve(r)).transpose()?);
                }
                port.update_channel(guild, *id, &fields, &reason).await?;
            }
            OpKind::ReorderChannels { scopes } => self.reorder_channels(scopes, &reason).await?,
            OpKind::DeleteChannel { id } => port.delete_channel(guild, *id, &reason).await?,
            OpKind::DeleteRole { id } => port.delete_role(guild, *id, &reason).await?,
        }
        Ok(())
    }
}

fn role_fields(a: &RoleAttrs) -> RoleFields {
    RoleFields {
        name: a.name.clone(),
        color: a.color,
        hoist: a.hoist,
        mentionable: a.mentionable,
        permissions: a.permissions,
    }
}
