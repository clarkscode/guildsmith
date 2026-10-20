//! Planning the kinds beyond roles and channels (US9, data-model §4–5). Each kind is planned
//! only when its live state was read (`LiveExtras` field is `Some`), which the app does for the
//! kinds the spec lists.
//!
//! References (`#channel`, `@Role`, or ids) resolve against the spec first, so an item can use
//! a channel or role created earlier in the same plan, and then against the live server.

mod automod;
mod events;
mod expressions;
mod invites;
mod messages;
mod onboarding;
mod settings;
mod webhooks;

use std::collections::BTreeMap;

use super::ops::{FieldChange, Op, OpKind, Ref, Risk, WriteId, Written};
use super::order::Phase;
use super::{Builder, category_key, channel_key};
use crate::id::Id;
use crate::live::LiveExtras;
use crate::snapshot::LiveChannelKind;
use crate::spec::SpecPath;
use crate::validate::discord_text_name;

/// blake3 of the spec's files, by spec path (`emoji[0].image`), read by the app.
pub type SpecAssets = BTreeMap<String, String>;

/// A spec channel and how it can be referred to.
struct ChannelEntry {
    name: String,
    stored: String,
    category: bool,
    r: Ref,
}

/// Resolves `#channel` and `@Role` references (spec first, then live).
pub(crate) struct Refs {
    channels: Vec<ChannelEntry>,
}

impl Refs {
    pub fn new(b: &Builder<'_>) -> Self {
        let mut channels = Vec::new();
        for (i, cat) in b.spec.categories.iter().enumerate() {
            channels.push(ChannelEntry {
                name: cat.name.clone(),
                stored: cat.name.clone(),
                category: true,
                r: b.m.categories[i].map_or_else(|| Ref::New(category_key(&cat.name)), Ref::Id),
            });
            for (j, ch) in cat.channels.iter().enumerate() {
                channels.push(ChannelEntry {
                    name: ch.name.clone(),
                    stored: crate::matching::stored_channel_name(ch),
                    category: false,
                    r: b.m.category_channels[i][j]
                        .map_or_else(|| Ref::New(channel_key(Some(&cat.name), &ch.name)), Ref::Id),
                });
            }
        }
        for (i, ch) in b.spec.channels.iter().enumerate() {
            channels.push(ChannelEntry {
                name: ch.name.clone(),
                stored: crate::matching::stored_channel_name(ch),
                category: false,
                r: b.m.channels[i].map_or_else(|| Ref::New(channel_key(None, &ch.name)), Ref::Id),
            });
        }
        Refs { channels }
    }
}

impl Builder<'_> {
    /// A channel reference: `#name`, a plain name or an id. Categories don't count.
    pub(crate) fn channel_ref(&self, refs: &Refs, input: &str) -> Result<Ref, String> {
        let t = input.trim();
        if let Ok(id) = t.parse::<Id>() {
            return Ok(Ref::Id(id));
        }
        let name = t.strip_prefix('#').unwrap_or(t);
        let text = discord_text_name(name);
        let hits: Vec<&ChannelEntry> = refs
            .channels
            .iter()
            .filter(|c| !c.category && (c.name == name || c.stored == name || c.stored == text))
            .collect();
        match hits.as_slice() {
            [one] => return Ok(one.r.clone()),
            [] => {}
            _ => return Err(format!("`{input}` matches {} channels in the spec; use an id", hits.len())),
        }
        let live: Vec<Id> = self
            .s
            .channels
            .iter()
            .filter(|c| matches!(c.kind, LiveChannelKind::Known(_)) && (c.name == name || c.name == text))
            .map(|c| c.id)
            .collect();
        match live.as_slice() {
            [one] => Ok(Ref::Id(*one)),
            [] => Err(format!("`{input}` is not a channel in the spec or on the server")),
            _ => Err(format!("`{input}` matches {} channels on the server; use an id", live.len())),
        }
    }

    /// A role reference: `@Name`, a plain name, `@everyone` or an id.
    pub(crate) fn role_ref(&self, input: &str) -> Result<Ref, String> {
        let t = input.trim();
        if t == "@everyone" {
            return Ok(Ref::Id(self.s.guild.id));
        }
        if let Ok(id) = t.parse::<Id>() {
            return Ok(Ref::Id(id));
        }
        let name = t.strip_prefix('@').unwrap_or(t);
        if let Some(r) = self.role_refs.get(name) {
            return Ok(r.clone());
        }
        let live: Vec<Id> = self.s.roles.iter().filter(|r| r.name == name).map(|r| r.id).collect();
        match live.as_slice() {
            [one] => Ok(Ref::Id(*one)),
            [] => Err(format!("`{input}` is not a role in the spec or on the server")),
            _ => Err(format!("`{input}` matches {} roles on the server; use an id", live.len())),
        }
    }

    /// Resolves a list, recording each failure against `path`.
    pub(crate) fn refs_or_err(
        &mut self,
        path: &SpecPath,
        inputs: &[String],
        f: impl Fn(&Self, &str) -> Result<Ref, String>,
    ) -> Option<Vec<Ref>> {
        let mut out = Vec::with_capacity(inputs.len());
        let mut ok = true;
        for i in inputs {
            match f(self, i) {
                Ok(r) => out.push(r),
                Err(e) => {
                    self.errors.push(format!("{path}: {e}"));
                    ok = false;
                }
            }
        }
        ok.then_some(out)
    }

    pub(crate) fn op(&mut self, phase: Phase, summary: String, risk: Risk, target: Ref, kind: OpKind) {
        self.push(phase, Op { summary, risk, target, replace_group: None, kind });
    }

    /// A delete and create sharing a replace group, deleting first.
    pub(crate) fn replace(&mut self, phase: Phase, delete: (String, Ref, OpKind), create: (String, OpKind)) {
        let group = self.next_group;
        self.next_group += 1;
        let (summary, target, kind) = delete;
        self.push(phase, Op { summary, risk: Risk::Destructive, target, replace_group: Some(group), kind });
        let (summary, kind) = create;
        let key = match &kind {
            OpKind::CreateAutomod { key, .. }
            | OpKind::CreateExpression { key, .. }
            | OpKind::CreateInvite { key, .. }
            | OpKind::CreateWebhook { key, .. } => key.clone(),
            _ => String::new(),
        };
        self.push(phase, Op { summary, risk: Risk::Safe, target: Ref::New(key), replace_group: Some(group), kind });
    }

    /// Whether the spec was exported from another server: its ids and codes are that server's,
    /// so one that isn't found here means "make it", not "it was deleted".
    pub(crate) fn foreign(&self) -> bool {
        self.spec.guild.id.is_some_and(|g| g != self.s.guild.id)
    }

    pub(crate) fn write_id(&mut self, path: SpecPath, field: &'static str, value: Written) {
        self.write_ids.push(WriteId { path, field, value });
    }

    /// Plans every kind whose live state was read.
    pub(crate) fn kinds(&mut self, x: &LiveExtras, assets: &SpecAssets) {
        let refs = Refs::new(self);
        settings::plan(self, &refs, x, assets);
        expressions::plan(self, x, assets);
        webhooks::plan(self, &refs, x, assets);
        automod::plan(self, &refs, x);
        onboarding::plan(self, &refs, x);
        events::plan(self, &refs, x, assets);
        invites::plan(self, &refs, x);
        messages::plan(self, &refs, x);
    }
}

/// `old → new` for a field, if they differ.
#[allow(clippy::needless_pass_by_value)] // numbers and strings alike
pub(crate) fn change(out: &mut Vec<FieldChange>, field: &str, old: impl ToString, new: impl ToString) -> bool {
    let (old, new) = (old.to_string(), new.to_string());
    if old == new {
        return false;
    }
    out.push(FieldChange { field: field.to_owned(), old, new });
    true
}

/// A field whose details stay out of the summary (lists, files, user text).
pub(crate) fn changed(out: &mut Vec<FieldChange>, field: &str) {
    out.push(FieldChange { field: field.to_owned(), old: String::new(), new: String::new() });
}

/// The desired file's hash, or an error that the app didn't read it.
pub(crate) fn spec_hash<'a>(b: &mut Builder<'_>, assets: &'a SpecAssets, path: &SpecPath) -> Option<&'a String> {
    let h = assets.get(&path.to_string());
    if h.is_none() {
        b.errors.push(format!("{path}: the file wasn't read"));
    }
    h
}

/// Summaries for a change list: fields with details show `old → new`.
pub(crate) fn summary(head: String, changes: &[FieldChange]) -> String {
    let parts: Vec<String> = changes
        .iter()
        .map(|c| if c.old.is_empty() && c.new.is_empty() { c.field.clone() } else { c.to_string() })
        .collect();
    if parts.is_empty() { head } else { format!("{head}: {}", parts.join(", ")) }
}
