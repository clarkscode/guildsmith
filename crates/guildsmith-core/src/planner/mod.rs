//! The planner: spec + snapshot + matches → ordered ops (data-model §3, research R7–R10).

pub mod kinds;
pub mod ops;
pub mod order;
pub mod positions;
pub mod simulate;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::diff::{self, describe_overwrites};
use crate::id::Id;
use crate::matching::{Matches, stored_channel_name};
use crate::snapshot::{Completeness, LiveChannel, LiveChannelKind, LiveOverwrite, OverwriteKind, Snapshot};
use crate::spec::{CategorySpec, ChannelKind, ChannelSpec, GuildSpec, OverwriteSpec, Target};

pub use kinds::SpecAssets;
pub use ops::{
    ChannelAttrs, ChannelSlot, FieldChange, Op, OpKind, OverwriteOp, Ref, Risk, RoleAttrs, RoleSlot, Scope, ScopeOrder,
    WriteId, Written,
};
pub use order::Phase;

/// More role creates than this in one apply gets a warning (research R9).
pub const ROLE_CREATE_WARNING: usize = 50;
/// Reads per snapshot: the guild (with roles), its channels and the bot's member record.
pub const SNAPSHOT_READS: u32 = 3;

/// A reviewed, ordered set of changes (data-model §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// `gsp_` + 32 hex characters (research R4).
    pub hash: String,
    pub ops: Vec<Op>,
    /// Live items the spec doesn't mention and won't delete.
    pub unmanaged: Vec<String>,
    pub warnings: Vec<String>,
    pub estimated_requests: u32,
    pub risk: Risk,
    pub snapshot_fingerprint: String,
    /// Ids to write into the spec file after apply (research E9); not ops.
    pub write_ids: Vec<WriteId>,
}

impl Plan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

pub(crate) fn role_key(name: &str) -> String {
    format!("role:{name}")
}

pub(crate) fn category_key(name: &str) -> String {
    format!("category:{name}")
}

pub(crate) fn channel_key(category: Option<&str>, name: &str) -> String {
    format!("channel:{}/{name}", category.unwrap_or(""))
}

/// `#name` for text-like channels, the plain name otherwise.
#[must_use]
pub fn channel_display(kind: LiveChannelKind, name: &str) -> String {
    match kind {
        LiveChannelKind::Known(k) if k.is_text_like() => format!("#{name}"),
        _ => name.to_owned(),
    }
}

fn summarize(head: String, changes: &[FieldChange]) -> String {
    if changes.is_empty() {
        return head;
    }
    let parts: Vec<String> = changes
        .iter()
        .map(|c| match c.field.as_str() {
            "permissions" | "overwrites" | "tags" => c.field.clone(),
            _ => c.to_string(),
        })
        .collect();
    format!("{head}: {}", parts.join(", "))
}

/// Builds the ops for one spec against one snapshot.
pub(crate) struct Builder<'a> {
    spec: &'a GuildSpec,
    s: &'a Snapshot,
    m: &'a Matches,
    pub ops: Vec<(Phase, Op)>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub unmanaged: Vec<String>,
    /// Spec role name → reference, for `@Name` overwrite targets.
    role_refs: BTreeMap<&'a str, Ref>,
    /// Live channels that a replace deletes and recreates.
    replaced: BTreeSet<Id>,
    next_group: u32,
    pub write_ids: Vec<WriteId>,
}

impl<'a> Builder<'a> {
    pub fn new(spec: &'a GuildSpec, s: &'a Snapshot, m: &'a Matches) -> Self {
        let role_refs = spec
            .roles
            .iter()
            .zip(&m.roles)
            .map(|(r, id)| (r.name.as_str(), id.map_or_else(|| Ref::New(role_key(&r.name)), Ref::Id)))
            .collect();
        Builder {
            spec,
            s,
            m,
            ops: Vec::new(),
            errors: Vec::new(),
            warnings: m.warnings.clone(),
            unmanaged: Vec::new(),
            role_refs,
            replaced: BTreeSet::new(),
            next_group: 0,
            write_ids: Vec::new(),
        }
    }

    fn push(&mut self, phase: Phase, op: Op) {
        self.ops.push((phase, op));
    }

    fn category_ref(&self, i: usize) -> Ref {
        self.m.categories[i].map_or_else(|| Ref::New(category_key(&self.spec.categories[i].name)), Ref::Id)
    }

    /// Resolves overwrite targets to references, sorted canonically.
    fn resolve(&mut self, list: &[OverwriteSpec], label: &str) -> Vec<OverwriteOp> {
        let mut out = Vec::with_capacity(list.len());
        for o in list {
            let (kind, target) = match &o.target {
                Target::Everyone => (OverwriteKind::Role, Ref::Id(self.s.guild.id)),
                Target::RoleName(n) => {
                    let Some(r) = self.role_refs.get(n.as_str()) else {
                        self.errors.push(format!("{label}: overwrite target @{n} is not a role in the spec"));
                        continue;
                    };
                    (OverwriteKind::Role, r.clone())
                }
                Target::RoleId(id) => {
                    if self.s.role(*id).is_none() {
                        self.errors.push(format!("{label}: overwrite target role:{id} is not a role on this server"));
                    }
                    (OverwriteKind::Role, Ref::Id(*id))
                }
                Target::Member(id) => (OverwriteKind::Member, Ref::Id(*id)),
            };
            out.push(OverwriteOp { kind, target, allow: o.allow, deny: o.deny });
        }
        out.sort();
        out
    }

    // ---- roles -------------------------------------------------------------------------------

    pub fn roles(&mut self) {
        for (i, r) in self.spec.roles.iter().enumerate() {
            let everyone = r.name == "@everyone";
            let Some(live) = self.m.roles[i].and_then(|id| self.s.role(id)) else {
                let key = role_key(&r.name);
                let op = Op {
                    summary: format!("create role @{}", r.name),
                    risk: Risk::Safe,
                    target: Ref::New(key.clone()),
                    replace_group: None,
                    kind: OpKind::CreateRole { key, attrs: diff::role_create_attrs(r) },
                };
                self.push(Phase::Roles, op);
                continue;
            };
            let (changes, attrs) = diff::role_changes(r, live, everyone);
            if changes.is_empty() {
                continue;
            }
            if live.managed && changes.iter().any(|c| c.field != "permissions") {
                self.warnings.push(format!(
                    "@{} is a managed role: only its permissions can change; preflight refuses the rest",
                    r.name
                ));
            }
            let name = if everyone { "@everyone".to_owned() } else { format!("@{}", live.name) };
            let summary = summarize(format!("update role {name}"), &changes);
            let kind = OpKind::UpdateRole { id: live.id, changes, attrs };
            let op = Op { summary, risk: Risk::Safe, target: Ref::Id(live.id), replace_group: None, kind };
            self.push(Phase::Roles, op);
        }
    }

    pub fn reorder_roles(&mut self) {
        let mut order = Vec::new();
        let mut live_order = Vec::new();
        let mut needs = false;
        for (i, r) in self.spec.roles.iter().enumerate() {
            if r.name == "@everyone" || r.managed {
                continue;
            }
            let position = r.position.map(i64::from);
            if let Some(id) = self.m.roles[i] {
                order.push(RoleSlot { role: Ref::Id(id), position });
                live_order.push((id, position));
            } else {
                needs = true;
                order.push(RoleSlot { role: Ref::New(role_key(&r.name)), position });
            }
        }
        needs |= !positions::roles_in_order(self.s, &live_order);
        if needs && (order.len() > 1 || order.iter().any(|o| o.position.is_some())) {
            let summary = format!("reorder roles ({})", order.len());
            let op = Op {
                summary,
                risk: Risk::Safe,
                target: Ref::Id(self.s.guild.id),
                replace_group: None,
                kind: OpKind::ReorderRoles { order },
            };
            self.push(Phase::RoleOrder, op);
        }
    }

    // ---- categories ----------------------------------------------------------------------------

    pub fn categories(&mut self) {
        for (i, c) in self.spec.categories.iter().enumerate() {
            let label = format!("category {}", c.name);
            let desired = c.overwrites.as_ref().map(|o| self.resolve(o, &label));
            let Some(live) = self.m.categories[i].and_then(|id| self.s.channel(id)) else {
                let key = category_key(&c.name);
                let attrs = ChannelAttrs {
                    name: Some(c.name.clone()),
                    kind: Some(LiveChannelKind::Category),
                    ..ChannelAttrs::default()
                };
                let kind = OpKind::CreateChannel { key: key.clone(), parent: None, attrs, overwrites: desired };
                let summary = format!("create category {}", c.name);
                let op = Op { summary, risk: Risk::Safe, target: Ref::New(key), replace_group: None, kind };
                self.push(Phase::Categories, op);
                continue;
            };
            let mut changes = Vec::new();
            let mut attrs = ChannelAttrs::default();
            if c.name != live.name {
                changes.push(FieldChange { field: "name".into(), old: live.name.clone(), new: c.name.clone() });
                attrs.name = Some(c.name.clone());
            }
            let overwrites = self.overwrite_change(desired, &live.overwrites, &mut changes);
            if changes.is_empty() {
                continue;
            }
            let summary = summarize(format!("update category {}", live.name), &changes);
            let kind = OpKind::UpdateChannel { id: live.id, changes, attrs, parent: None, overwrites };
            let op = Op { summary, risk: Risk::Safe, target: Ref::Id(live.id), replace_group: None, kind };
            self.push(Phase::Categories, op);
        }
    }

    /// Records an overwrite change and returns the list to send, or `None` when unchanged.
    fn overwrite_change(
        &self,
        desired: Option<Vec<OverwriteOp>>,
        live: &[LiveOverwrite],
        changes: &mut Vec<FieldChange>,
    ) -> Option<Vec<OverwriteOp>> {
        let desired = desired?;
        if diff::same_overwrites(&desired, live) {
            return None;
        }
        changes.push(FieldChange {
            field: "overwrites".into(),
            old: describe_overwrites(self.s, live.iter().map(|o| (o.kind, Ref::Id(o.id), o.allow, o.deny))),
            new: describe_overwrites(self.s, desired.iter().map(|o| (o.kind, o.target.clone(), o.allow, o.deny))),
        });
        Some(desired)
    }

    // ---- channels ------------------------------------------------------------------------------

    /// The overwrites a channel should end up with, or `None` when they aren't managed.
    fn channel_overwrites(&mut self, cat: Option<usize>, ch: &ChannelSpec, label: &str) -> Option<Vec<OverwriteOp>> {
        if ch.sync_permissions
            && let Some(ci) = cat
        {
            let c: &CategorySpec = &self.spec.categories[ci];
            if let Some(list) = &c.overwrites {
                return Some(self.resolve(list, label));
            }
            // The category's overwrites aren't managed: copy what it has now.
            let live = self.m.categories[ci].and_then(|id| self.s.channel(id));
            let mut list: Vec<OverwriteOp> = live
                .into_iter()
                .flat_map(|l| &l.overwrites)
                .map(|o| OverwriteOp { kind: o.kind, target: Ref::Id(o.id), allow: o.allow, deny: o.deny })
                .collect();
            list.sort();
            return Some(list);
        }
        ch.overwrites.as_ref().map(|o| self.resolve(o, label))
    }

    pub fn channels(&mut self) {
        let spec = self.spec;
        let mut all: Vec<(Option<usize>, &ChannelSpec, Option<Id>)> = Vec::new();
        for (ci, c) in spec.categories.iter().enumerate() {
            for (j, ch) in c.channels.iter().enumerate() {
                all.push((Some(ci), ch, self.m.category_channels[ci][j]));
            }
        }
        for (j, ch) in spec.channels.iter().enumerate() {
            all.push((None, ch, self.m.channels[j]));
        }
        for (cat, ch, matched) in all {
            let cat_name = cat.map(|ci| spec.categories[ci].name.as_str());
            let label = match cat_name {
                Some(c) => format!("channel {c}/{}", ch.name),
                None => format!("channel {}", ch.name),
            };
            let parent = cat.map(|ci| self.category_ref(ci));
            let overwrites = self.channel_overwrites(cat, ch, &label);
            if let Some(live) = matched.and_then(|id| self.s.channel(id)) {
                self.update_channel(cat_name, ch, live.id, parent, overwrites, &label);
            } else {
                self.create_channel(cat_name, ch, parent, overwrites, None);
            }
        }
    }

    fn create_channel(
        &mut self,
        cat: Option<&str>,
        ch: &ChannelSpec,
        parent: Option<Ref>,
        overwrites: Option<Vec<OverwriteOp>>,
        replace_group: Option<u32>,
    ) {
        let key = channel_key(cat, &stored_channel_name(ch));
        let kind_word = ch.kind.as_str();
        let name = channel_display(LiveChannelKind::Known(ch.kind), &stored_channel_name(ch));
        let summary = match (cat, replace_group) {
            (_, Some(_)) => format!("replace channel {name}: create it as {kind_word}"),
            (Some(c), None) => format!("create {kind_word} channel {name} in {c}"),
            (None, None) => format!("create {kind_word} channel {name}"),
        };
        let kind =
            OpKind::CreateChannel { key: key.clone(), parent, attrs: diff::channel_create_attrs(ch), overwrites };
        self.push(Phase::Channels, Op { summary, risk: Risk::Safe, target: Ref::New(key), replace_group, kind });
    }

    fn update_channel(
        &mut self,
        cat: Option<&str>,
        ch: &ChannelSpec,
        id: Id,
        parent: Option<Ref>,
        overwrites: Option<Vec<OverwriteOp>>,
        label: &str,
    ) {
        let live = self.s.channel(id).expect("matched channel is live");
        let (mut changes, mut attrs) = diff::channel_changes(ch, live);
        if live.kind != LiveChannelKind::Known(ch.kind) {
            // Only text ↔ announcement changes in place, and only with the NEWS feature (R10).
            let text_news =
                |k: LiveChannelKind| matches!(k, LiveChannelKind::Known(ChannelKind::Text | ChannelKind::Announcement));
            let in_place = text_news(live.kind)
                && text_news(LiveChannelKind::Known(ch.kind))
                && self.s.guild.features.contains("NEWS");
            if in_place {
                let kind_change =
                    FieldChange { field: "kind".into(), old: kind_word(live.kind), new: ch.kind.to_string() };
                changes.insert(0, kind_change);
                attrs.kind = Some(LiveChannelKind::Known(ch.kind));
            } else if ch.replace || self.spec.prune.channels {
                self.replace_channel(cat, ch, id, parent, overwrites);
                return;
            } else {
                self.errors.push(format!(
                    "{label}: changing a {} channel to {} needs delete + create; set `replace = true` on it \
                     (or `prune.channels = true`) to allow that",
                    kind_word(live.kind),
                    ch.kind
                ));
                return;
            }
        }
        let desired_parent = parent.clone();
        let live_parent = live.parent_id.map(Ref::Id);
        let parent_change = if desired_parent == live_parent {
            None
        } else {
            changes.push(FieldChange {
                field: "category".into(),
                old: live
                    .parent_id
                    .and_then(|p| self.s.channel(p))
                    .map_or_else(|| "none".to_owned(), |p| p.name.clone()),
                new: cat.unwrap_or("none").to_owned(),
            });
            Some(desired_parent)
        };
        let overwrites = self.overwrite_change(overwrites, &live.overwrites, &mut changes);
        if changes.is_empty() {
            return;
        }
        let summary = summarize(format!("update channel {}", channel_display(live.kind, &live.name)), &changes);
        let kind = OpKind::UpdateChannel { id, changes, attrs, parent: parent_change, overwrites };
        self.push(Phase::Channels, Op { summary, risk: Risk::Safe, target: Ref::Id(id), replace_group: None, kind });
    }

    fn replace_channel(
        &mut self,
        cat: Option<&str>,
        ch: &ChannelSpec,
        id: Id,
        parent: Option<Ref>,
        overwrites: Option<Vec<OverwriteOp>>,
    ) {
        let live = self.s.channel(id).expect("matched channel is live");
        self.next_group += 1;
        let group = self.next_group;
        self.replaced.insert(id);
        self.create_channel(cat, ch, parent, overwrites, Some(group));
        let summary = format!(
            "replace channel {}: delete the {} channel ({})",
            channel_display(live.kind, &live.name),
            kind_word(live.kind),
            id
        );
        let op = Op {
            summary,
            risk: Risk::Destructive,
            target: Ref::Id(id),
            replace_group: Some(group),
            kind: OpKind::DeleteChannel { id },
        };
        self.push(Phase::DeleteChannels, op);
    }

    /// Whether a matched channel ends up in a different parent than it has now.
    fn moved(&self, live_id: Id, parent: Option<&Ref>) -> bool {
        let live_parent = self.s.channel(live_id).and_then(|c| c.parent_id);
        match parent {
            Some(Ref::Id(p)) => live_parent != Some(*p),
            Some(Ref::New(_)) => true,
            None => live_parent.and_then(|p| self.s.channel(p)).is_some_and(|p| p.kind == LiveChannelKind::Category),
        }
    }

    fn scope_order(&self, scope: Scope, items: Vec<(Option<Id>, String)>, parent: Option<&Ref>) -> Option<ScopeOrder> {
        let mut order = Vec::new();
        let mut live = Vec::new();
        let mut needs = false;
        for (matched, key) in items {
            if let Some(id) = matched.filter(|id| !self.replaced.contains(id)) {
                needs |= self.moved(id, parent);
                order.push(ChannelSlot { channel: Ref::Id(id), position: None });
                live.push((id, None));
            } else {
                needs = true;
                order.push(ChannelSlot { channel: Ref::New(key), position: None });
            }
        }
        needs |= !positions::channels_in_order(self.s, &live);
        (needs && order.len() > 1).then_some(ScopeOrder { scope, order })
    }

    pub fn reorder_channels(&mut self) {
        let spec = self.spec;
        let mut scopes = Vec::new();

        // Categories at the server level.
        let mut order = Vec::new();
        let mut live = Vec::new();
        let mut needs = false;
        for (i, c) in spec.categories.iter().enumerate() {
            let position = c.position.map(i32::from);
            if let Some(id) = self.m.categories[i] {
                order.push(ChannelSlot { channel: Ref::Id(id), position });
                live.push((id, position));
            } else {
                needs = true;
                order.push(ChannelSlot { channel: Ref::New(category_key(&c.name)), position });
            }
        }
        needs |= !positions::channels_in_order(self.s, &live);
        if needs && (order.len() > 1 || order.iter().any(|o| o.position.is_some())) {
            scopes.push(ScopeOrder { scope: Scope::Categories, order });
        }

        for (ci, c) in spec.categories.iter().enumerate() {
            let items = c
                .channels
                .iter()
                .zip(&self.m.category_channels[ci])
                .map(|(ch, id)| (*id, channel_key(Some(&c.name), &stored_channel_name(ch))))
                .collect();
            let parent = self.category_ref(ci);
            scopes.extend(self.scope_order(Scope::Category(parent.clone()), items, Some(&parent)));
        }
        let items = spec
            .channels
            .iter()
            .zip(&self.m.channels)
            .map(|(ch, id)| (*id, channel_key(None, &stored_channel_name(ch))))
            .collect();
        scopes.extend(self.scope_order(Scope::TopLevel, items, None));

        if !scopes.is_empty() {
            let n: usize = scopes.iter().map(|s| s.order.len()).sum();
            let op = Op {
                summary: format!("reorder channels ({n})"),
                risk: Risk::Safe,
                target: Ref::Id(self.s.guild.id),
                replace_group: None,
                kind: OpKind::ReorderChannels { scopes },
            };
            self.push(Phase::ChannelOrder, op);
        }
    }

    // ---- unmanaged items and prune ---------------------------------------------------------------

    pub fn prune(&mut self) {
        let s = self.s;
        let prune = self.spec.prune;
        let matched: BTreeSet<Id> = self
            .m
            .roles
            .iter()
            .chain(&self.m.categories)
            .chain(self.m.category_channels.iter().flatten())
            .chain(&self.m.channels)
            .flatten()
            .copied()
            .collect();

        let mut channels: Vec<_> = s.channels.iter().filter(|c| !matched.contains(&c.id)).collect();
        channels.sort_by_key(|c| (c.position, c.id));
        // Deleting a category moves its remaining children to the top level, where they could
        // collide by name with managed channels. Only prune a category the plan empties.
        let stays = |c: &&LiveChannel| {
            !matched.contains(&c.id) && (!prune.channels || matches!(c.kind, LiveChannelKind::Other(_)))
        };
        for c in &channels {
            let kept: Vec<&LiveChannel> =
                s.channels.iter().filter(|k| k.parent_id == Some(c.id)).filter(stays).collect();
            match c.kind {
                LiveChannelKind::Other(n) => {
                    self.unmanaged.push(format!("channel {} (type {n}, not supported), id {}", c.name, c.id));
                }
                LiveChannelKind::Category if prune.categories && kept.is_empty() => {
                    self.delete_channel(c.id, &format!("category {}", c.name), Phase::DeleteCategories);
                }
                LiveChannelKind::Category if prune.categories => {
                    self.warnings.push(format!(
                        "not pruning category {}: it still holds {} channel(s) the spec doesn't list; \
                         set prune.channels or list them",
                        c.name,
                        kept.len()
                    ));
                    self.unmanaged.push(format!("category {}, id {}", c.name, c.id));
                }
                LiveChannelKind::Category => self.unmanaged.push(format!("category {}, id {}", c.name, c.id)),
                LiveChannelKind::Known(_) if prune.channels => {
                    self.delete_channel(
                        c.id,
                        &format!("channel {}", channel_display(c.kind, &c.name)),
                        Phase::DeleteChannels,
                    );
                }
                LiveChannelKind::Known(_) => {
                    self.unmanaged.push(format!("channel {}, id {}", channel_display(c.kind, &c.name), c.id));
                }
            }
        }

        let mut roles: Vec<_> = s.roles.iter().filter(|r| r.id != s.guild.id && !matched.contains(&r.id)).collect();
        roles.sort_by_key(|r| (std::cmp::Reverse(r.position), r.id));
        let owner = s.bot.user_id == s.guild.owner_id;
        for r in roles {
            let protected = r.managed || (!owner && r.position >= s.bot.top_role_position);
            if prune.roles && !protected {
                let op = Op {
                    summary: format!("delete role @{} ({})", r.name, r.id),
                    risk: Risk::Destructive,
                    target: Ref::Id(r.id),
                    replace_group: None,
                    kind: OpKind::DeleteRole { id: r.id },
                };
                self.push(Phase::DeleteRoles, op);
            } else {
                if prune.roles {
                    let why = if r.managed { "it is managed" } else { "it is at or above the bot's highest role" };
                    self.warnings.push(format!("not pruning role @{}: {why}", r.name));
                }
                self.unmanaged.push(format!("role @{}, id {}", r.name, r.id));
            }
        }
        if s.completeness == Completeness::MaybeIncomplete {
            self.warnings
                .push("the bot is not an administrator: channels hidden from it aren't listed or planned".to_owned());
        }
    }

    fn delete_channel(&mut self, id: Id, what: &str, phase: Phase) {
        let op = Op {
            summary: format!("delete {what} ({id})"),
            risk: Risk::Destructive,
            target: Ref::Id(id),
            replace_group: None,
            kind: OpKind::DeleteChannel { id },
        };
        self.push(phase, op);
    }
}

fn kind_word(k: LiveChannelKind) -> String {
    match k {
        LiveChannelKind::Category => "category".to_owned(),
        LiveChannelKind::Known(k) => k.as_str().to_owned(),
        LiveChannelKind::Other(n) => format!("type {n}"),
    }
}
