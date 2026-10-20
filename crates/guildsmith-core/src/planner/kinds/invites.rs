//! `[[invites]]`: only invites the bot made are considered. With a `code`, the invite is that
//! one; without, it is matched by signature (channel, max age, max uses, temporary, roles).
//! Invites can't be edited, and an expired or deleted one is made again only with
//! `recreate = true` (data-model §4).

use std::collections::BTreeSet;

use super::Refs;
use crate::live::{InviteInfo, LiveExtras};
use crate::planner::Builder;
use crate::planner::ops::{OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::invites::InviteSpec;
use crate::time;

fn key(i: usize) -> String {
    format!("invite:{i}")
}

fn expired(l: &InviteInfo, now: i64) -> bool {
    l.expires_at.as_deref().and_then(time::parse_rfc3339).is_some_and(|t| t <= now)
        || (l.max_uses > 0 && l.uses >= l.max_uses)
}

/// The spec invite's signature against a live invite.
fn same(w: &InviteSpec, channel: &Ref, roles: &[Ref], l: &InviteInfo) -> bool {
    let mut want: Vec<&Ref> = roles.iter().collect();
    want.sort();
    let mut have: Vec<Ref> = l.roles.iter().map(|r| Ref::Id(*r)).collect();
    have.sort();
    *channel == Ref::Id(l.channel_id)
        && w.max_age == l.max_age
        && w.max_uses == l.max_uses
        && w.temporary == l.temporary
        && want.into_iter().cloned().collect::<Vec<_>>() == have
}

fn describe(w: &InviteSpec, channel: &Ref) -> String {
    let age = if w.max_age == 0 { "never expires".to_owned() } else { format!("expires after {} s", w.max_age) };
    format!("invite to {channel} ({age})")
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras) {
    let Some(all) = &x.invites else { return };
    let (bot, guild) = (b.s.bot.user_id, b.s.guild.id);
    let live: Vec<&InviteInfo> = all.iter().filter(|i| i.inviter_id == Some(bot) && !expired(i, x.now)).collect();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for (i, w) in b.spec.invites.iter().enumerate() {
        let path = SpecPath::root().key("invites").index(i);
        let channel = match b.channel_ref(refs, &w.channel) {
            Ok(c) => c,
            Err(e) => {
                b.errors.push(format!("{path}.channel: {e}"));
                continue;
            }
        };
        let Some(roles) = b.refs_or_err(&path.clone().key("roles"), &w.roles, Builder::role_ref) else { continue };
        let create = |k: String| OpKind::CreateInvite {
            key: k,
            channel: channel.clone(),
            max_age: w.max_age,
            max_uses: w.max_uses,
            temporary: w.temporary,
            roles: roles.clone(),
        };
        let by_signature = || live.iter().find(|l| !used.contains(l.code.as_str()) && same(w, &channel, &roles, l));
        let found = match &w.code {
            Some(code) => {
                live.iter().find(|l| l.code == *code).or_else(|| if b.foreign() { by_signature() } else { None })
            }
            None => by_signature(),
        };
        match (found, &w.code) {
            (Some(l), code) => {
                used.insert(&l.code);
                if code.as_deref() != Some(l.code.as_str()) {
                    b.write_id(path.clone(), "code", Written::Code(l.code.clone()));
                }
                if !same(w, &channel, &roles, l) {
                    if w.recreate {
                        b.replace(
                            Phase::Invites,
                            (
                                format!("delete invite {} (its settings change)", l.code),
                                Ref::Id(guild),
                                OpKind::DeleteInvite { code: l.code.clone() },
                            ),
                            (format!("create {}", describe(w, &channel)), create(key(i))),
                        );
                        b.write_id(path, "code", Written::New(key(i)));
                    } else {
                        b.warnings.push(format!(
                            "{path}: invite {} doesn't match the spec, and invites can't be changed; set recreate = true \
                             to replace it",
                            l.code
                        ));
                    }
                }
            }
            (None, Some(code)) if !w.recreate && !b.foreign() => b.warnings.push(format!(
                "{path}: invite {code} has expired or was deleted; set recreate = true to make a new one"
            )),
            (None, _) => {
                b.op(
                    Phase::Invites,
                    format!("create {}", describe(w, &channel)),
                    Risk::Safe,
                    Ref::New(key(i)),
                    create(key(i)),
                );
                b.write_id(path, "code", Written::New(key(i)));
            }
        }
    }
    for l in live.iter().filter(|l| !used.contains(l.code.as_str())) {
        if b.spec.prune.invites {
            b.op(
                Phase::DeleteInvites,
                format!("delete invite {}", l.code),
                Risk::Destructive,
                Ref::Id(guild),
                OpKind::DeleteInvite { code: l.code.clone() },
            );
        } else if !b.spec.invites.is_empty() {
            b.unmanaged.push(format!("invite {} made by the bot (not in spec, prune.invites = false)", l.code));
        }
    }
}
