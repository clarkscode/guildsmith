//! Effective channel permissions, explained step by step (research R13).
//!
//! Follows Discord's documented order: owner → base (`@everyone` + roles) → administrator →
//! `@everyone` overwrite → role overwrites → member overwrite → implicit rules. Preflight,
//! `permissions_explain` and the planner's visibility checks all use this one function.

use serde::Serialize;

use crate::id::Id;
use crate::permissions::{Permission, PermissionSet};
use crate::snapshot::{LiveChannel, LiveChannelKind, OverwriteKind, Snapshot};

/// Whose permissions to compute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    Member(MemberInfo),
    /// A role on its own: computed as a member holding only that role (plus `@everyone`).
    Role(Id),
}

/// What the explainer needs to know about a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    pub id: Id,
    pub role_ids: Vec<Id>,
    /// Timed out (communication disabled) right now.
    pub timed_out: bool,
}

/// One step of the calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Owner,
    Base,
    Administrator,
    EveryoneOverwrite,
    RoleOverwrites,
    MemberOverwrite,
    Implicit,
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub step: StepKind,
    /// The role, overwrite or rule responsible, e.g. `overwrite for @everyone`.
    pub source: String,
    pub added: PermissionSet,
    pub removed: PermissionSet,
}

/// The result of an explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explanation {
    pub allowed: PermissionSet,
    /// Known permissions that are not allowed.
    pub denied: PermissionSet,
    pub steps: Vec<Step>,
    /// For a queried permission: the index in `steps` of the last step that changed it.
    pub decided_by: Option<usize>,
}

/// Why an explanation couldn't be computed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExplainError {
    #[error("channel {0} is not in the snapshot")]
    UnknownChannel(Id),
    #[error("role {0} is not in the snapshot")]
    UnknownRole(Id),
}

/// Permissions that only matter once a member can send messages.
const NEEDS_SEND: &[Permission] =
    &[Permission::SendTtsMessages, Permission::MentionEveryone, Permission::EmbedLinks, Permission::AttachFiles];

/// Permissions that only matter once a member can connect to voice.
const NEEDS_CONNECT: &[Permission] = &[
    Permission::Speak,
    Permission::MuteMembers,
    Permission::DeafenMembers,
    Permission::MoveMembers,
    Permission::UseVad,
    Permission::PrioritySpeaker,
    Permission::Stream,
    Permission::RequestToSpeak,
    Permission::UseSoundboard,
    Permission::UseExternalSounds,
    Permission::UseEmbeddedActivities,
    Permission::SetVoiceChannelStatus,
];

struct Calc<'a> {
    snapshot: &'a Snapshot,
    perms: PermissionSet,
    steps: Vec<Step>,
    query: Option<Permission>,
    decided_by: Option<usize>,
}

impl Calc<'_> {
    fn apply(&mut self, step: StepKind, source: String, next: PermissionSet) {
        let added = next.difference(self.perms);
        let removed = self.perms.difference(next);
        if let Some(q) = self.query
            && (added.contains(q) || removed.contains(q))
        {
            self.decided_by = Some(self.steps.len());
        }
        self.steps.push(Step { step, source, added, removed });
        self.perms = next;
    }

    fn role_name(&self, id: Id) -> String {
        if id == self.snapshot.guild.id {
            return "@everyone".to_owned();
        }
        self.snapshot.role(id).map_or_else(|| format!("role:{id}"), |r| format!("@{}", r.name))
    }

    fn finish(self) -> Explanation {
        Explanation {
            allowed: self.perms,
            denied: PermissionSet::all_known().difference(self.perms),
            steps: self.steps,
            decided_by: self.decided_by,
        }
    }
}

/// Computes `who`'s effective permissions in `channel`. With `query`, records which step
/// decided that one permission.
pub fn explain(
    snapshot: &Snapshot,
    channel: Id,
    who: &Who,
    query: Option<Permission>,
) -> Result<Explanation, ExplainError> {
    let ch = snapshot.channel(channel).ok_or(ExplainError::UnknownChannel(channel))?;
    let member = match who {
        Who::Member(m) => m.clone(),
        Who::Role(id) => {
            snapshot.role(*id).ok_or(ExplainError::UnknownRole(*id))?;
            MemberInfo { id: Id(0), role_ids: vec![*id], timed_out: false }
        }
    };
    let mut calc = Calc { snapshot, perms: PermissionSet::EMPTY, steps: Vec::new(), query, decided_by: None };

    // 1. The owner has every permission.
    if matches!(who, Who::Member(_)) && member.id == snapshot.guild.owner_id {
        calc.apply(StepKind::Owner, "server owner".to_owned(), PermissionSet::all_known());
        return Ok(calc.finish());
    }

    // 2. Base: @everyone plus every role the member has.
    let mut base = snapshot.everyone().map_or(PermissionSet::EMPTY, |r| r.permissions);
    let mut sources = vec!["@everyone".to_owned()];
    for id in &member.role_ids {
        if *id == snapshot.guild.id {
            continue;
        }
        if let Some(role) = snapshot.role(*id) {
            base = base.union(role.permissions);
            sources.push(format!("@{}", role.name));
        }
    }
    calc.apply(StepKind::Base, format!("roles {}", sources.join(", ")), base);

    // 3. Administrator in the base means everything; overwrites are skipped.
    if base.contains(Permission::Administrator) {
        calc.apply(StepKind::Administrator, "administrator permission".to_owned(), PermissionSet::all_known());
        return Ok(calc.finish());
    }

    apply_overwrites(&mut calc, ch, &member, who);
    apply_implicit(&mut calc, ch, &member);
    Ok(calc.finish())
}

/// Steps 4–6: the `@everyone`, role and member overwrites on the channel.
fn apply_overwrites(calc: &mut Calc<'_>, ch: &LiveChannel, member: &MemberInfo, who: &Who) {
    let snapshot = calc.snapshot;
    // 4. The @everyone overwrite: deny, then allow.
    if let Some(o) = ch.overwrites.iter().find(|o| o.kind == OverwriteKind::Role && o.id == snapshot.guild.id) {
        let next = calc.perms.difference(o.deny).union(o.allow);
        calc.apply(StepKind::EveryoneOverwrite, "overwrite for @everyone".to_owned(), next);
    }

    // 5. Every role overwrite for the member's roles, combined: deny, then allow.
    let (mut deny, mut allow) = (PermissionSet::EMPTY, PermissionSet::EMPTY);
    let mut names = Vec::new();
    for o in &ch.overwrites {
        if o.kind == OverwriteKind::Role && o.id != snapshot.guild.id && member.role_ids.contains(&o.id) {
            deny = deny.union(o.deny);
            allow = allow.union(o.allow);
            names.push(calc.role_name(o.id));
        }
    }
    if !names.is_empty() {
        let next = calc.perms.difference(deny).union(allow);
        calc.apply(StepKind::RoleOverwrites, format!("overwrites for {}", names.join(", ")), next);
    }

    // 6. The member's own overwrite.
    if matches!(who, Who::Member(_))
        && let Some(o) = ch.overwrites.iter().find(|o| o.kind == OverwriteKind::Member && o.id == member.id)
    {
        let next = calc.perms.difference(o.deny).union(o.allow);
        calc.apply(StepKind::MemberOverwrite, format!("overwrite for member:{}", member.id), next);
    }
}

/// Step 7: implicit rules that remove permissions which can't apply.
fn apply_implicit(calc: &mut Calc<'_>, ch: &LiveChannel, member: &MemberInfo) {
    // 7. Implicit rules.
    let p = calc.perms;
    if !p.contains(Permission::ViewChannel) {
        if !p.is_empty() {
            calc.apply(StepKind::Implicit, "no view_channel: nothing else applies".to_owned(), PermissionSet::EMPTY);
        }
        return;
    }
    if member.timed_out {
        let kept: PermissionSet =
            [Permission::ViewChannel, Permission::ReadMessageHistory].into_iter().filter(|x| p.contains(*x)).collect();
        calc.apply(StepKind::Implicit, "timed out: only view_channel and read_message_history".to_owned(), kept);
        return;
    }
    let is_voice = matches!(ch.kind, LiveChannelKind::Known(k) if k.is_voice_like());
    if !p.contains(Permission::SendMessages) && NEEDS_SEND.iter().any(|x| p.contains(*x)) {
        let mut next = p;
        for x in NEEDS_SEND {
            next.remove(*x);
        }
        calc.apply(StepKind::Implicit, "no send_messages: message permissions don't apply".to_owned(), next);
    }
    let p = calc.perms;
    if is_voice && !p.contains(Permission::Connect) && NEEDS_CONNECT.iter().any(|x| p.contains(*x)) {
        let mut next = p;
        for x in NEEDS_CONNECT {
            next.remove(*x);
        }
        calc.apply(StepKind::Implicit, "no connect: voice permissions don't apply".to_owned(), next);
    }
}
