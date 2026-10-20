//! A pure, in-memory apply: what the server looks like after a plan runs. It follows the same
//! Discord rules as `FakeDiscord` (new roles at position 1, new channels last, reorders that
//! permute the slots managed items occupy), so property tests can check that plans converge
//! without any I/O (constitution III).

mod kinds;

use std::collections::BTreeMap;

use super::ops::{ChannelAttrs, Op, OpKind, OverwriteOp, Ref, RoleAttrs, Scope, ScopeOrder};
use super::positions;
use crate::id::Id;
use crate::live::LiveExtras;
use crate::snapshot::{LiveChannel, LiveChannelKind, LiveOverwrite, LiveRole, LiveTag, Snapshot};

/// Why a simulated op couldn't run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("simulated op `{summary}` failed: {reason}")]
pub struct SimulateError {
    pub summary: String,
    pub reason: String,
}

struct Sim {
    s: Snapshot,
    x: LiveExtras,
    created: BTreeMap<String, Id>,
    /// Invite codes made by the plan, by op key.
    codes: BTreeMap<String, String>,
    next_id: u64,
}

impl Sim {
    fn resolve(&self, r: &Ref) -> Result<Id, String> {
        match r {
            Ref::Id(id) => Ok(*id),
            Ref::New(key) => self.created.get(key).copied().ok_or_else(|| format!("{key} was not created")),
        }
    }

    fn fresh_id(&mut self) -> Id {
        self.next_id += 1;
        Id(self.next_id)
    }

    fn overwrites(&self, list: &[OverwriteOp]) -> Result<Vec<LiveOverwrite>, String> {
        list.iter()
            .map(|o| Ok(LiveOverwrite { id: self.resolve(&o.target)?, kind: o.kind, allow: o.allow, deny: o.deny }))
            .collect()
    }

    fn set_role(r: &mut LiveRole, a: &RoleAttrs) {
        if let Some(v) = &a.name {
            r.name.clone_from(v);
        }
        if let Some(v) = a.color {
            r.color = v;
        }
        if let Some(v) = a.hoist {
            r.hoist = v;
        }
        if let Some(v) = a.mentionable {
            r.mentionable = v;
        }
        if let Some(v) = a.permissions {
            r.permissions = v;
        }
    }

    fn set_channel(&mut self, id: Id, a: &ChannelAttrs) {
        let mut tags = a.tags.as_ref().map(|t| {
            t.iter().map(|t| (t.id, t.name.clone(), t.emoji.clone(), t.moderated.unwrap_or(false))).collect::<Vec<_>>()
        });
        if let Some(list) = &mut tags {
            for t in list.iter_mut().filter(|t| t.0.is_none()) {
                t.0 = Some(self.fresh_id());
            }
        }
        let Some(c) = self.s.channels.iter_mut().find(|c| c.id == id) else { return };
        if let Some(v) = &a.name {
            c.name.clone_from(v);
        }
        if let Some(v) = a.kind {
            c.kind = v;
        }
        if let Some(v) = &a.topic {
            c.topic = (!v.is_empty()).then(|| v.clone());
        }
        if let Some(v) = a.nsfw {
            c.nsfw = v;
        }
        if let Some(v) = a.slowmode {
            c.slowmode = v;
        }
        if let Some(v) = a.bitrate {
            c.bitrate = Some(v);
        }
        if let Some(v) = a.user_limit {
            c.user_limit = Some(v);
        }
        if let Some(v) = &a.rtc_region {
            c.rtc_region.clone_from(v);
        }
        if let Some(list) = tags {
            c.tags = list
                .into_iter()
                .map(|(id, name, emoji, moderated)| LiveTag { id: id.expect("filled above"), name, emoji, moderated })
                .collect();
        }
        if let Some(v) = &a.default_reaction {
            c.default_reaction.clone_from(v);
        }
    }

    fn create_channel(
        &mut self,
        key: &str,
        parent: Option<&Ref>,
        attrs: &ChannelAttrs,
        overwrites: Option<&[OverwriteOp]>,
    ) -> Result<(), String> {
        let id = self.fresh_id();
        let parent_id = parent.map(|p| self.resolve(p)).transpose()?;
        let position = self.s.channels.iter().map(|c| c.position).max().map_or(0, |m| m + 1);
        let overwrites = overwrites.map(|o| self.overwrites(o)).transpose()?.unwrap_or_default();
        self.s.channels.push(LiveChannel {
            id,
            name: String::new(),
            kind: attrs.kind.unwrap_or(LiveChannelKind::Category),
            parent_id,
            position,
            topic: None,
            nsfw: false,
            slowmode: 0,
            bitrate: None,
            user_limit: None,
            rtc_region: None,
            tags: Vec::new(),
            default_reaction: None,
            overwrites,
        });
        self.set_channel(id, attrs);
        self.created.insert(key.to_owned(), id);
        Ok(())
    }

    fn reorder_channels(&mut self, scopes: &[ScopeOrder]) -> Result<(), String> {
        for scope in scopes {
            if let Scope::Category(r) = &scope.scope {
                self.resolve(r)?;
            }
            let order: Vec<(Id, Option<i32>)> = scope
                .order
                .iter()
                .map(|o| Ok((self.resolve(&o.channel)?, o.position)))
                .collect::<Result<_, String>>()?;
            for (id, p) in positions::channel_positions(&self.s, &order) {
                if let Some(c) = self.s.channels.iter_mut().find(|c| c.id == id) {
                    c.position = p;
                }
            }
        }
        Ok(())
    }

    fn run(&mut self, op: &Op) -> Result<(), String> {
        match &op.kind {
            OpKind::CreateRole { key, attrs } => {
                let id = self.fresh_id();
                for r in self.s.roles.iter_mut().filter(|r| r.id != self.s.guild.id) {
                    r.position += 1;
                }
                let mut role = LiveRole {
                    id,
                    name: String::new(),
                    color: 0,
                    secondary_color: None,
                    tertiary_color: None,
                    hoist: false,
                    mentionable: false,
                    managed: false,
                    permissions: crate::PermissionSet::EMPTY,
                    position: 1,
                    tags: None,
                };
                Self::set_role(&mut role, attrs);
                self.s.roles.push(role);
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateRole { id, attrs, .. } => {
                let r = self.s.roles.iter_mut().find(|r| r.id == *id).ok_or("role not found")?;
                Self::set_role(r, attrs);
            }
            OpKind::ReorderRoles { order } => {
                let order: Vec<(Id, Option<i64>)> =
                    order.iter().map(|o| Ok((self.resolve(&o.role)?, o.position))).collect::<Result<_, String>>()?;
                for (id, p) in positions::role_positions(&self.s, &order) {
                    if let Some(r) = self.s.roles.iter_mut().find(|r| r.id == id) {
                        r.position = p;
                    }
                }
            }
            OpKind::CreateChannel { key, parent, attrs, overwrites } => {
                self.create_channel(key, parent.as_ref(), attrs, overwrites.as_deref())?;
            }
            OpKind::UpdateChannel { id, attrs, parent, overwrites, .. } => {
                let parent = parent.as_ref().map(|p| p.as_ref().map(|r| self.resolve(r)).transpose()).transpose()?;
                let overwrites = overwrites.as_deref().map(|o| self.overwrites(o)).transpose()?;
                let c = self.s.channels.iter_mut().find(|c| c.id == *id).ok_or("channel not found")?;
                if let Some(p) = parent {
                    c.parent_id = p;
                }
                if let Some(o) = overwrites {
                    c.overwrites = o;
                }
                self.set_channel(*id, attrs);
            }
            OpKind::ReorderChannels { scopes } => self.reorder_channels(scopes)?,
            OpKind::DeleteChannel { id } => {
                self.s.channels.retain(|c| c.id != *id);
                for c in &mut self.s.channels {
                    if c.parent_id == Some(*id) {
                        c.parent_id = None;
                    }
                }
            }
            OpKind::DeleteRole { id } => {
                self.s.roles.retain(|r| r.id != *id);
                for c in &mut self.s.channels {
                    c.overwrites.retain(|o| o.id != *id);
                }
            }
            other => self.run_kind(other)?,
        }
        Ok(())
    }
}

fn run_all(snapshot: &Snapshot, extras: &LiveExtras, ops: &[Op]) -> Result<Sim, SimulateError> {
    let max = snapshot.roles.iter().map(|r| r.id.0).chain(snapshot.channels.iter().map(|c| c.id.0)).max().unwrap_or(0);
    let mut sim = Sim {
        s: snapshot.clone(),
        x: extras.clone(),
        created: BTreeMap::new(),
        codes: BTreeMap::new(),
        next_id: max.max(1 << 40),
    };
    for op in ops {
        sim.run(op).map_err(|reason| SimulateError { summary: op.summary.clone(), reason })?;
    }
    Ok(sim)
}

/// The snapshot after running `ops` in order.
pub fn simulate(snapshot: &Snapshot, ops: &[Op]) -> Result<Snapshot, SimulateError> {
    run_all(snapshot, &LiveExtras::default(), ops).map(|sim| sim.s)
}

/// [`simulate`], also returning the id each create op made, by its key.
pub fn simulate_keyed(snapshot: &Snapshot, ops: &[Op]) -> Result<(Snapshot, BTreeMap<String, Id>), SimulateError> {
    run_all(snapshot, &LiveExtras::default(), ops).map(|sim| (sim.s, sim.created))
}

/// The server, and every other kind's live state, after running `ops` in order. Kinds not read
/// (`None` in `extras`) stay unread.
pub fn simulate_with(
    snapshot: &Snapshot,
    extras: &LiveExtras,
    ops: &[Op],
) -> Result<(Snapshot, LiveExtras), SimulateError> {
    run_all(snapshot, extras, ops).map(|sim| (sim.s, sim.x))
}
