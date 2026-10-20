//! `[[automod]]`: rules matched by id, then name. A rule whose trigger type changes is
//! replaced (Discord can't change it); unlisted rules are deleted only with `prune.automod`.

use std::collections::BTreeSet;

use super::{Refs, change, changed, summary};
use crate::id::Id;
use crate::live::LiveExtras;
use crate::planner::Builder;
use crate::planner::ops::{FieldChange, OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::automod::{Action, AutomodRule};

fn key(name: &str) -> String {
    format!("automod:{name}")
}

/// The spec rule with roles and channels resolved.
fn resolve(b: &mut Builder<'_>, refs: &Refs, path: &SpecPath, r: &AutomodRule<String>) -> Option<AutomodRule<Ref>> {
    let roles = b.refs_or_err(&path.clone().key("exempt_roles"), &r.exempt_roles, Builder::role_ref);
    let channels =
        b.refs_or_err(&path.clone().key("exempt_channels"), &r.exempt_channels, |b, c| b.channel_ref(refs, c));
    let mut actions = Vec::with_capacity(r.actions.len());
    for (i, a) in r.actions.iter().enumerate() {
        actions.push(match a {
            Action::Block { message } => Action::Block { message: message.clone() },
            Action::Alert { channel } => match b.channel_ref(refs, channel) {
                Ok(c) => Action::Alert { channel: c },
                Err(e) => {
                    b.errors.push(format!("{}: {e}", path.clone().key("actions").index(i)));
                    return None;
                }
            },
            Action::Timeout { duration } => Action::Timeout { duration: *duration },
            Action::BlockInteraction => Action::BlockInteraction,
        });
    }
    Some(AutomodRule {
        id: r.id,
        name: r.name.clone(),
        enabled: r.enabled,
        trigger: r.trigger.clone(),
        actions,
        exempt_roles: roles?,
        exempt_channels: channels?,
    })
}

fn as_refs(rule: &AutomodRule<Id>) -> AutomodRule<Ref> {
    rule.clone().map_refs(|id| Ok::<_, ()>(Ref::Id(id))).unwrap_or_else(|()| unreachable!("infallible"))
}

fn set<T: Ord + Clone>(v: &[T]) -> BTreeSet<T> {
    v.iter().cloned().collect()
}

fn changes(want: &AutomodRule<Ref>, live: &AutomodRule<Ref>) -> Vec<FieldChange> {
    let mut ch = Vec::new();
    change(&mut ch, "name", &live.name, &want.name);
    change(&mut ch, "enabled", live.enabled, want.enabled);
    if want.trigger != live.trigger {
        changed(&mut ch, "trigger");
    }
    if want.actions != live.actions {
        changed(&mut ch, "actions");
    }
    if set(&want.exempt_roles) != set(&live.exempt_roles) {
        changed(&mut ch, "exempt_roles");
    }
    if set(&want.exempt_channels) != set(&live.exempt_channels) {
        changed(&mut ch, "exempt_channels");
    }
    ch
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras) {
    let Some(live) = &x.automod else { return };
    let mut used = BTreeSet::new();
    for (i, r) in b.spec.automod.iter().enumerate() {
        let path = SpecPath::root().key("automod").index(i);
        let found =
            r.id.and_then(|id| live.iter().find(|l| l.id == Some(id)))
                .or_else(|| live.iter().find(|l| l.name == r.name && !used.contains(&l.id)));
        let Some(want) = resolve(b, refs, &path, r) else { continue };
        let Some(l) = found else {
            if r.id.is_some() {
                b.warnings.push(format!("{path}: automod rule {} no longer exists; it will be created again", r.name));
            }
            b.op(
                Phase::Automod,
                format!("create automod rule {}", r.name),
                Risk::Safe,
                Ref::New(key(&r.name)),
                OpKind::CreateAutomod { key: key(&r.name), rule: want },
            );
            b.write_id(path, "id", Written::New(key(&r.name)));
            continue;
        };
        let id = l.id.unwrap_or(Id(0));
        used.insert(l.id);
        if r.id != Some(id) {
            b.write_id(path, "id", Written::Id(id));
        }
        let have = as_refs(l);
        if want.trigger.code() != have.trigger.code() {
            b.replace(
                Phase::Automod,
                (
                    format!("delete automod rule {} (trigger type changes)", l.name),
                    Ref::Id(id),
                    OpKind::DeleteAutomod { id },
                ),
                (format!("create automod rule {}", r.name), OpKind::CreateAutomod { key: key(&r.name), rule: want }),
            );
            continue;
        }
        let ch = changes(&want, &have);
        if !ch.is_empty() {
            b.op(
                Phase::Automod,
                summary(format!("update automod rule {}", l.name), &ch),
                Risk::Safe,
                Ref::Id(id),
                OpKind::UpdateAutomod { id, changes: ch, rule: want },
            );
        }
    }
    for l in live.iter().filter(|l| !used.contains(&l.id)) {
        let Some(id) = l.id else { continue };
        if b.spec.prune.automod {
            b.op(
                Phase::DeleteAutomod,
                format!("delete automod rule {}", l.name),
                Risk::Destructive,
                Ref::Id(id),
                OpKind::DeleteAutomod { id },
            );
        } else {
            b.unmanaged.push(format!("automod rule {} (not in spec, prune.automod = false)", l.name));
        }
    }
}
