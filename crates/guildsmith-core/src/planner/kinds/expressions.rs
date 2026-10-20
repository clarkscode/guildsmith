//! `[[emoji]]`, `[[stickers]]`, `[[sounds]]`: matched by id, then name. A different file is a
//! replace (Discord can't change an expression's file), which is destructive: the old one's
//! uses stop showing it.

use std::collections::BTreeSet;

use super::{SpecAssets, change, changed, spec_hash, summary};
use crate::asset::Asset;
use crate::id::Id;
use crate::live::{ExpressionInfo, ExpressionKind, LiveExtras, asset_key};
use crate::planner::Builder;
use crate::planner::ops::{ExpressionSet, FieldChange, FileOp, NewExpressionOp, OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::expressions::Volume;

/// One spec item of any expression kind, flattened.
struct Want<'a> {
    id: Option<Id>,
    name: &'a str,
    file: &'a Asset,
    file_field: &'static str,
    roles: &'a [String],
    description: &'a str,
    tags: &'a str,
    volume: Option<Volume>,
    emoji: Option<&'a String>,
}

fn key(kind: ExpressionKind, name: &str) -> String {
    format!("{}:{name}", kind.name())
}

fn list(kind: ExpressionKind) -> &'static str {
    match kind {
        ExpressionKind::Emoji => "emoji",
        ExpressionKind::Sticker => "stickers",
        ExpressionKind::Sound => "sounds",
    }
}

fn items(spec: &crate::spec::GuildSpec, kind: ExpressionKind) -> Vec<Want<'_>> {
    match kind {
        ExpressionKind::Emoji => spec
            .emoji
            .iter()
            .map(|e| Want {
                id: e.id,
                name: &e.name,
                file: &e.image,
                file_field: "image",
                roles: &e.roles,
                description: "",
                tags: "",
                volume: None,
                emoji: None,
            })
            .collect(),
        ExpressionKind::Sticker => spec
            .stickers
            .iter()
            .map(|s| Want {
                id: s.id,
                name: &s.name,
                file: &s.image,
                file_field: "image",
                roles: &[],
                description: &s.description,
                tags: &s.tags,
                volume: None,
                emoji: None,
            })
            .collect(),
        ExpressionKind::Sound => spec
            .sounds
            .iter()
            .map(|s| Want {
                id: s.id,
                name: &s.name,
                file: &s.sound,
                file_field: "sound",
                roles: &[],
                description: "",
                tags: "",
                volume: s.volume,
                emoji: s.emoji.as_ref(),
            })
            .collect(),
    }
}

fn prune(b: &Builder<'_>, kind: ExpressionKind) -> bool {
    match kind {
        ExpressionKind::Emoji => b.spec.prune.emoji,
        ExpressionKind::Sticker => b.spec.prune.stickers,
        ExpressionKind::Sound => b.spec.prune.sounds,
    }
}

/// Field changes for a matched expression (the file aside).
fn edits(w: &Want<'_>, roles: &[Ref], l: &ExpressionInfo, ch: &mut Vec<FieldChange>) -> ExpressionSet {
    let mut set = ExpressionSet::default();
    if change(ch, "name", &l.name, w.name) {
        set.name = Some(w.name.to_owned());
    }
    match l.kind {
        ExpressionKind::Emoji => {
            let live: BTreeSet<Ref> = l.roles.iter().map(|r| Ref::Id(*r)).collect();
            if roles.iter().cloned().collect::<BTreeSet<_>>() != live {
                changed(ch, "roles");
                set.roles = Some(roles.to_vec());
            }
        }
        ExpressionKind::Sticker => {
            if change(ch, "description", l.description.as_deref().unwrap_or(""), w.description) {
                set.description = Some(w.description.to_owned());
            }
            if change(ch, "tags", l.tags.as_deref().unwrap_or(""), w.tags) {
                set.tags = Some(w.tags.to_owned());
            }
        }
        ExpressionKind::Sound => {
            let want = w.volume.map_or(1.0, |v| v.0);
            if w.volume.is_some() && (l.volume.unwrap_or(1.0) - want).abs() > 1e-6 {
                change(ch, "volume", l.volume.unwrap_or(1.0), want);
                set.volume = w.volume;
            }
            if w.emoji != l.emoji.as_ref() {
                change(ch, "emoji", l.emoji.as_deref().unwrap_or("none"), w.emoji.map_or("none", String::as_str));
                set.emoji = Some(w.emoji.cloned());
            }
        }
    }
    set
}

fn plan_kind(b: &mut Builder<'_>, live: &[ExpressionInfo], kind: ExpressionKind, x: &LiveExtras, assets: &SpecAssets) {
    let spec = b.spec;
    let mut used = BTreeSet::new();
    for (i, want) in items(spec, kind).iter().enumerate() {
        let path = SpecPath::root().key(list(kind)).index(i);
        let found = want
            .id
            .and_then(|id| live.iter().find(|have| have.id == id))
            .or_else(|| live.iter().find(|have| have.name == want.name && !used.contains(&have.id)));
        let roles = b.refs_or_err(&path.clone().key("roles"), want.roles, Builder::role_ref).unwrap_or_default();
        let Some(hash) = spec_hash(b, assets, &path.clone().key(want.file_field)).cloned() else { continue };
        let op_key = key(kind, want.name);
        let new = || OpKind::CreateExpression {
            kind,
            key: op_key.clone(),
            new: Box::new(NewExpressionOp {
                name: want.name.to_owned(),
                file: FileOp { asset: want.file.clone(), hash: hash.clone() },
                roles: roles.clone(),
                description: want.description.to_owned(),
                tags: want.tags.to_owned(),
                volume: want.volume,
                emoji: want.emoji.cloned(),
            }),
        };
        let Some(have) = found else {
            b.op(
                Phase::Expressions,
                format!("create {} {}", kind.name(), want.name),
                Risk::Safe,
                Ref::New(op_key.clone()),
                new(),
            );
            b.write_id(path, "id", Written::New(op_key));
            continue;
        };
        used.insert(have.id);
        match x.asset_hashes.get(&asset_key(kind.name(), have.id)) {
            Some(h) if *h != hash => {
                b.replace(
                    Phase::Expressions,
                    (
                        format!("delete {} {} (its file changes)", kind.name(), have.name),
                        Ref::Id(have.id),
                        OpKind::DeleteExpression { kind, id: have.id },
                    ),
                    (format!("create {} {}", kind.name(), want.name), new()),
                );
                b.write_id(path, "id", Written::New(op_key));
                continue;
            }
            Some(_) => {}
            None => b.warnings.push(format!(
                "{path}: couldn't read the live file of {} {}; not compared",
                kind.name(),
                have.name
            )),
        }
        if want.id != Some(have.id) {
            b.write_id(path, "id", Written::Id(have.id));
        }
        let mut ch = Vec::new();
        let set = edits(want, &roles, have, &mut ch);
        if !ch.is_empty() {
            b.op(
                Phase::Expressions,
                summary(format!("update {} {}", kind.name(), have.name), &ch),
                Risk::Safe,
                Ref::Id(have.id),
                OpKind::UpdateExpression { kind, id: have.id, changes: ch, set },
            );
        }
    }
    for have in live.iter().filter(|have| !used.contains(&have.id)) {
        if prune(b, kind) {
            b.op(
                Phase::DeleteExpressions,
                format!("delete {} {}", kind.name(), have.name),
                Risk::Destructive,
                Ref::Id(have.id),
                OpKind::DeleteExpression { kind, id: have.id },
            );
        } else {
            b.unmanaged.push(format!("{} {} (not in spec, prune.{} = false)", kind.name(), have.name, list(kind)));
        }
    }
}

pub(super) fn plan(b: &mut Builder<'_>, x: &LiveExtras, assets: &SpecAssets) {
    let Some(live) = &x.expressions else { return };
    for kind in [ExpressionKind::Emoji, ExpressionKind::Sticker, ExpressionKind::Sound] {
        let mine: Vec<ExpressionInfo> = live.iter().filter(|e| e.kind == kind).cloned().collect();
        let listed = !items(b.spec, kind).is_empty() || prune(b, kind);
        if listed {
            plan_kind(b, &mine, kind, x, assets);
        }
    }
}
