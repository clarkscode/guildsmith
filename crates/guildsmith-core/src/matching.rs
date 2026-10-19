//! Matching spec items to live items (research R12, with the R6 visibility guard).
//!
//! Items match by `id` first, then by unique name within their scope: roles and categories at
//! the server level, channels within their parent. No state file is kept.

use std::collections::BTreeSet;

use crate::id::Id;
use crate::snapshot::{Completeness, LiveChannel, LiveChannelKind, Snapshot};
use crate::spec::{ChannelSpec, GuildSpec};
use crate::validate::discord_text_name;

/// Why a spec can't be planned. Every problem is collected before reporting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the spec can't be planned:\n- {}", .problems.join("\n- "))]
pub struct PlanError {
    pub problems: Vec<String>,
}

/// The live id each spec item matched, or `None` when the plan creates it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matches {
    /// Per `spec.roles` index. `@everyone` always matches the server id.
    pub roles: Vec<Option<Id>>,
    /// Per `spec.categories` index.
    pub categories: Vec<Option<Id>>,
    /// Per category, per channel in it.
    pub category_channels: Vec<Vec<Option<Id>>>,
    /// Per top-level `spec.channels` index.
    pub channels: Vec<Option<Id>>,
    pub warnings: Vec<String>,
}

/// The name Discord stores for a channel of this kind.
#[must_use]
pub fn stored_channel_name(ch: &ChannelSpec) -> String {
    if ch.kind.is_text_like() { discord_text_name(&ch.name) } else { ch.name.clone() }
}

struct Ctx<'a> {
    snapshot: &'a Snapshot,
    claimed: BTreeSet<Id>,
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl Ctx<'_> {
    fn incomplete(&self) -> bool {
        self.snapshot.completeness == Completeness::MaybeIncomplete
    }

    /// The id-match step. Returns `Some(id)` when the id is live, and decides what a missing id
    /// means: a hard error if the bot may not see everything, otherwise a stale-id warning.
    fn by_id(&mut self, label: &str, id: Option<Id>, exists: bool) -> Option<Id> {
        let id = id?;
        if exists {
            if !self.claimed.insert(id) {
                self.errors.push(format!("{label}: id {id} is used by more than one item in the spec"));
            }
            return Some(id);
        }
        if self.incomplete() {
            self.errors.push(format!(
                "{label} (id {id}) is missing or hidden from the bot; give the bot Administrator so it can see \
                 every channel, or remove the id if the item was deleted"
            ));
        } else {
            self.warnings.push(format!("{label}: id {id} no longer exists; matching by name instead (stale id)"));
        }
        None
    }

    /// The name-match step among unclaimed `candidates`. `prefer` breaks ties (same kind).
    fn by_name<'c>(
        &mut self,
        label: &str,
        candidates: impl Iterator<Item = (Id, &'c str, bool)>,
        name: &str,
    ) -> Option<Id> {
        let all: Vec<(Id, &str, bool)> =
            candidates.filter(|(id, n, _)| *n == name && !self.claimed.contains(id)).collect();
        let pick = match all.as_slice() {
            [] => return None,
            [one] => one.0,
            many => {
                let preferred: Vec<_> = many.iter().filter(|c| c.2).collect();
                if let [one] = preferred.as_slice() {
                    one.0
                } else {
                    let list: Vec<String> = many.iter().map(|c| format!("{} ({})", c.1, c.0)).collect();
                    self.errors.push(format!(
                        "{label} matches more than one live item: {}; add an `id` to pick one",
                        list.join(", ")
                    ));
                    return None;
                }
            }
        };
        self.claimed.insert(pick);
        Some(pick)
    }
}

fn is_category(c: &LiveChannel) -> bool {
    c.kind == LiveChannelKind::Category
}

fn match_roles(ctx: &mut Ctx<'_>, spec: &GuildSpec) -> Vec<Option<Id>> {
    let s = ctx.snapshot;
    let mut out = Vec::with_capacity(spec.roles.len());
    // Ids first, so a renamed role isn't claimed by name for another entry.
    let mut by_id = Vec::with_capacity(spec.roles.len());
    for r in &spec.roles {
        let label = format!("role @{}", r.name);
        if r.name == "@everyone" {
            if r.id.is_some_and(|id| id != s.guild.id) {
                ctx.errors.push("@everyone's id must be the server id".to_owned());
            }
            by_id.push(Some(s.guild.id));
            continue;
        }
        let exists = r.id.is_some_and(|id| id != s.guild.id && s.role(id).is_some());
        by_id.push(ctx.by_id(&label, r.id, exists));
    }
    for (r, id) in spec.roles.iter().zip(by_id) {
        let label = format!("role @{}", r.name);
        let matched = id.or_else(|| {
            let live = s.roles.iter().filter(|l| l.id != s.guild.id).map(|l| (l.id, l.name.as_str(), true));
            ctx.by_name(&label, live, &r.name)
        });
        if r.name != "@everyone" {
            check_managed(ctx, &label, r.managed, matched);
        }
        out.push(matched);
    }
    out
}

fn check_managed(ctx: &mut Ctx<'_>, label: &str, spec_managed: bool, matched: Option<Id>) {
    let live_managed = matched.and_then(|id| ctx.snapshot.role(id)).is_some_and(|l| l.managed);
    if live_managed && !spec_managed {
        ctx.errors.push(format!(
            "{label} is managed by an integration or bot; mark it `managed = true` to manage its permissions, \
             or remove it from the spec"
        ));
    } else if spec_managed && !live_managed {
        ctx.errors.push(format!("{label} has `managed = true` but no managed role with that name or id exists"));
    }
}

fn match_categories(ctx: &mut Ctx<'_>, spec: &GuildSpec) -> Vec<Option<Id>> {
    let s = ctx.snapshot;
    let by_id: Vec<Option<Id>> = spec
        .categories
        .iter()
        .map(|c| {
            let label = format!("category {}", c.name);
            let exists = c.id.and_then(|id| s.channel(id)).is_some_and(is_category);
            if let Some(id) = c.id
                && s.channel(id).is_some_and(|l| !is_category(l))
            {
                ctx.errors.push(format!("{label}: id {id} is a channel, not a category"));
                return None;
            }
            ctx.by_id(&label, c.id, exists)
        })
        .collect();
    spec.categories
        .iter()
        .zip(by_id)
        .map(|(c, id)| {
            id.or_else(|| {
                let live = s.channels.iter().filter(|l| is_category(l)).map(|l| (l.id, l.name.as_str(), true));
                ctx.by_name(&format!("category {}", c.name), live, &c.name)
            })
        })
        .collect()
}

fn channel_label(cat: Option<&str>, ch: &ChannelSpec) -> String {
    match cat {
        Some(c) => format!("channel {c}/{}", ch.name),
        None => format!("channel {}", ch.name),
    }
}

fn channel_by_id(ctx: &mut Ctx<'_>, cat: Option<&str>, ch: &ChannelSpec) -> Option<Id> {
    let label = channel_label(cat, ch);
    let live = ch.id.and_then(|id| ctx.snapshot.channel(id));
    if let (Some(id), Some(l)) = (ch.id, live)
        && is_category(l)
    {
        ctx.errors.push(format!("{label}: id {id} is a category, not a channel"));
        return None;
    }
    if let (Some(id), Some(LiveChannelKind::Other(n))) = (ch.id, live.map(|l| l.kind)) {
        ctx.errors.push(format!("{label}: id {id} is a channel type guildsmith doesn't manage ({n})"));
        return None;
    }
    ctx.by_id(&label, ch.id, live.is_some())
}

/// Where to look for a channel by name.
#[derive(Clone, Copy)]
enum NameScope {
    /// Inside this live category.
    Category(Id),
    /// Uncategorized channels.
    TopLevel,
    /// A category the plan creates: it has no live children yet.
    NewCategory,
}

/// Name matching for one scope.
fn channel_by_name(ctx: &mut Ctx<'_>, cat: Option<&str>, scope: NameScope, ch: &ChannelSpec) -> Option<Id> {
    let parent = match scope {
        NameScope::Category(id) => Some(id),
        NameScope::TopLevel => None,
        NameScope::NewCategory => return None,
    };
    let s = ctx.snapshot;
    let in_scope = |l: &LiveChannel| {
        !is_category(l)
            && match parent {
                Some(p) => l.parent_id == Some(p),
                None => l.parent_id.and_then(|p| s.channel(p)).is_none_or(|p| !is_category(p)),
            }
    };
    let name = stored_channel_name(ch);
    let live = s
        .channels
        .iter()
        .filter(|l| in_scope(l) && matches!(l.kind, LiveChannelKind::Known(_)))
        .map(|l| (l.id, l.name.as_str(), l.kind == LiveChannelKind::Known(ch.kind)));
    ctx.by_name(&channel_label(cat, ch), live, &name)
}

/// Matches every spec item, or reports every problem found.
pub fn match_spec(spec: &GuildSpec, snapshot: &Snapshot) -> Result<Matches, PlanError> {
    let mut ctx = Ctx { snapshot, claimed: BTreeSet::new(), errors: Vec::new(), warnings: Vec::new() };
    let roles = match_roles(&mut ctx, spec);
    let categories = match_categories(&mut ctx, spec);

    // Channel ids across every scope first, so a channel moved by id isn't claimed by name.
    let cat_ids: Vec<Vec<Option<Id>>> = spec
        .categories
        .iter()
        .map(|c| c.channels.iter().map(|ch| channel_by_id(&mut ctx, Some(&c.name), ch)).collect())
        .collect();
    let top_ids: Vec<Option<Id>> = spec.channels.iter().map(|ch| channel_by_id(&mut ctx, None, ch)).collect();

    let mut category_channels = Vec::with_capacity(spec.categories.len());
    for ((c, ids), parent) in spec.categories.iter().zip(cat_ids).zip(&categories) {
        let scope = parent.map_or(NameScope::NewCategory, NameScope::Category);
        let list = c
            .channels
            .iter()
            .zip(ids)
            .map(|(ch, id)| id.or_else(|| channel_by_name(&mut ctx, Some(&c.name), scope, ch)))
            .collect();
        category_channels.push(list);
    }
    let channels = spec
        .channels
        .iter()
        .zip(top_ids)
        .map(|(ch, id)| id.or_else(|| channel_by_name(&mut ctx, None, NameScope::TopLevel, ch)))
        .collect();

    if ctx.errors.is_empty() {
        Ok(Matches { roles, categories, category_channels, channels, warnings: ctx.warnings })
    } else {
        Err(PlanError { problems: ctx.errors })
    }
}
