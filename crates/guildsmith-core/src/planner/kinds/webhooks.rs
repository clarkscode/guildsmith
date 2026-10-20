//! `[[webhooks]]`: incoming webhooks matched by id, then name. Follower and application
//! webhooks are never touched. A created webhook's URL is reported once by apply (E8).

use std::collections::BTreeSet;

use super::{Refs, SpecAssets, change, changed, spec_hash, summary};
use crate::live::{LiveExtras, WebhookKind, asset_key};
use crate::planner::Builder;
use crate::planner::ops::{FileOp, OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;

fn key(name: &str) -> String {
    format!("webhook:{name}")
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras, assets: &SpecAssets) {
    let Some(all) = &x.webhooks else { return };
    let live: Vec<_> = all.iter().filter(|w| w.kind == WebhookKind::Incoming).collect();
    let mut used = BTreeSet::new();
    for (i, w) in b.spec.webhooks.iter().enumerate() {
        let path = SpecPath::root().key("webhooks").index(i);
        let channel = match b.channel_ref(refs, &w.channel) {
            Ok(c) => c,
            Err(e) => {
                b.errors.push(format!("{path}.channel: {e}"));
                continue;
            }
        };
        let avatar = match &w.avatar {
            Some(a) => {
                let Some(h) = spec_hash(b, assets, &path.clone().key("avatar")).cloned() else { continue };
                Some(FileOp { asset: a.clone(), hash: h })
            }
            None => None,
        };
        let found =
            w.id.and_then(|id| live.iter().find(|l| l.id == id))
                .or_else(|| live.iter().find(|l| l.name == w.name && !used.contains(&l.id)));
        let Some(l) = found else {
            b.op(
                Phase::Webhooks,
                format!("create webhook {} in {channel}", w.name),
                Risk::Safe,
                Ref::New(key(&w.name)),
                OpKind::CreateWebhook { key: key(&w.name), name: w.name.clone(), channel, avatar },
            );
            b.write_id(path, "id", Written::New(key(&w.name)));
            continue;
        };
        used.insert(l.id);
        if w.id != Some(l.id) {
            b.write_id(path, "id", Written::Id(l.id));
        }
        let mut ch = Vec::new();
        let name = change(&mut ch, "name", &l.name, &w.name).then(|| w.name.clone());
        let moved = (channel != Ref::Id(l.channel_id)).then_some(channel);
        if let Some(c) = &moved {
            change(&mut ch, "channel", l.channel_id, c);
        }
        let avatar =
            avatar.filter(|a| l.avatar.is_none() || x.asset_hashes.get(&asset_key("webhook", l.id)) != Some(&a.hash));
        if avatar.is_some() {
            changed(&mut ch, "avatar");
        }
        if !ch.is_empty() {
            b.op(
                Phase::Webhooks,
                summary(format!("update webhook {}", l.name), &ch),
                Risk::Safe,
                Ref::Id(l.id),
                OpKind::UpdateWebhook { id: l.id, changes: ch, name, channel: moved, avatar },
            );
        }
    }
    for l in live.iter().filter(|l| !used.contains(&l.id)) {
        if b.spec.prune.webhooks {
            b.op(
                Phase::DeleteWebhooks,
                format!("delete webhook {} (its URL stops working)", l.name),
                Risk::Destructive,
                Ref::Id(l.id),
                OpKind::DeleteWebhook { id: l.id },
            );
        } else if !b.spec.webhooks.is_empty() {
            b.unmanaged.push(format!("webhook {} (not in spec, prune.webhooks = false)", l.name));
        }
    }
}
