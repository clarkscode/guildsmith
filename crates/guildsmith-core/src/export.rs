//! Snapshot → spec: what `export` writes (FR-003/FR-004). Pure; the app layer fetches the
//! snapshot and the writer formats the result.

use std::collections::BTreeMap;

use crate::id::Id;
use crate::snapshot::{Completeness, LiveChannel, LiveChannelKind, LiveOverwrite, LiveRole, OverwriteKind, Snapshot};
use crate::spec::{
    CategorySpec, ChannelSpec, Color, ForumTag, GuildInfo, GuildSpec, OverwriteSpec, PruneSpec, RoleSpec, Target,
};

/// Facts the spec can't hold, written as comments next to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportNotes {
    /// Managed roles, as `"Name" <why> role, id <id>`.
    pub managed_roles: Vec<String>,
    /// Gradient colors per role id: `(secondary, tertiary)`.
    pub gradients: BTreeMap<Id, (Option<u32>, Option<u32>)>,
    /// Live items guildsmith doesn't describe (unsupported channel kinds).
    pub unmanaged: Vec<String>,
    /// The bot can't see every channel (research R6).
    pub incomplete: bool,
}

fn role_spec(r: &LiveRole) -> RoleSpec {
    RoleSpec {
        id: Some(r.id),
        name: r.name.clone(),
        color: (r.color != 0).then_some(Color(r.color)),
        hoist: Some(r.hoist),
        mentionable: Some(r.mentionable),
        permissions: Some(r.permissions),
        position: None,
        managed: false,
    }
}

fn managed_reason(r: &LiveRole) -> &'static str {
    match &r.tags {
        Some(t) if t.bot_id.is_some() => "bot",
        Some(t) if t.premium_subscriber => "server booster",
        Some(t) if t.integration_id.is_some() => "integration",
        _ => "managed",
    }
}

/// Maps a live overwrite to a spec target. Role names are used when unique and the role is in
/// the spec; otherwise `role:<id>`.
fn target(snapshot: &Snapshot, o: &LiveOverwrite) -> Target {
    match o.kind {
        OverwriteKind::Member => Target::Member(o.id),
        OverwriteKind::Role if o.id == snapshot.guild.id => Target::Everyone,
        OverwriteKind::Role => match snapshot.role(o.id) {
            Some(r) if !r.managed && snapshot.roles.iter().filter(|x| x.name == r.name).count() == 1 => {
                Target::RoleName(r.name.clone())
            }
            _ => Target::RoleId(o.id),
        },
    }
}

fn overwrites(snapshot: &Snapshot, list: &[LiveOverwrite]) -> Vec<OverwriteSpec> {
    let mut out: Vec<OverwriteSpec> =
        list.iter().map(|o| OverwriteSpec { target: target(snapshot, o), allow: o.allow, deny: o.deny }).collect();
    out.sort_by(|a, b| a.target.cmp(&b.target));
    out
}

fn same_overwrites(a: &[LiveOverwrite], b: &[LiveOverwrite]) -> bool {
    let key = |l: &[LiveOverwrite]| {
        let mut v: Vec<_> = l.iter().map(|o| (o.kind, o.id, o.allow, o.deny)).collect();
        v.sort();
        v
    };
    key(a) == key(b)
}

fn channel_spec(snapshot: &Snapshot, c: &LiveChannel, parent: Option<&LiveChannel>) -> Option<ChannelSpec> {
    let LiveChannelKind::Known(kind) = c.kind else {
        return None;
    };
    let synced = parent.is_some_and(|p| same_overwrites(&p.overwrites, &c.overwrites));
    let text_like = kind.is_text_like();
    let voice_like = kind.is_voice_like();
    Some(ChannelSpec {
        id: Some(c.id),
        name: c.name.clone(),
        kind,
        topic: if voice_like { None } else { c.topic.clone() },
        nsfw: Some(c.nsfw),
        slowmode: (text_like && kind != crate::spec::ChannelKind::Announcement).then_some(c.slowmode),
        bitrate: if voice_like { c.bitrate } else { None },
        user_limit: if voice_like { c.user_limit } else { None },
        rtc_region: voice_like.then(|| c.rtc_region.clone().unwrap_or_else(|| "auto".to_owned())),
        tags: kind.is_forum_like().then(|| {
            c.tags
                .iter()
                .map(|t| ForumTag {
                    id: Some(t.id),
                    name: t.name.clone(),
                    emoji: t.emoji.clone(),
                    moderated: Some(t.moderated),
                })
                .collect()
        }),
        default_reaction: if kind.is_forum_like() { c.default_reaction.clone() } else { None },
        sync_permissions: synced,
        overwrites: if synced { None } else { Some(overwrites(snapshot, &c.overwrites)) },
        replace: false,
    })
}

fn unmanaged_note(c: &LiveChannel, n: u8) -> String {
    format!("\"{}\" (channel type {n}), id {}", c.name, c.id)
}

/// Builds the spec export writes, plus the notes that become comments.
#[must_use]
pub fn to_spec(snapshot: &Snapshot) -> (GuildSpec, ExportNotes) {
    let s = snapshot;
    let mut notes =
        ExportNotes { incomplete: s.completeness == Completeness::MaybeIncomplete, ..ExportNotes::default() };

    let mut roles: Vec<&LiveRole> = s.roles.iter().filter(|r| r.id != s.guild.id).collect();
    roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    let mut role_specs = Vec::new();
    for r in roles {
        if r.managed {
            notes.managed_roles.push(format!("\"{}\" {} role, id {}", r.name, managed_reason(r), r.id));
            continue;
        }
        if r.secondary_color.is_some() || r.tertiary_color.is_some() {
            notes.gradients.insert(r.id, (r.secondary_color, r.tertiary_color));
        }
        role_specs.push(role_spec(r));
    }
    if let Some(everyone) = s.everyone() {
        role_specs.push(RoleSpec {
            name: "@everyone".to_owned(),
            permissions: Some(everyone.permissions),
            ..RoleSpec::default()
        });
    }

    let by_position = |a: &&LiveChannel, b: &&LiveChannel| a.position.cmp(&b.position).then(a.id.cmp(&b.id));
    let mut cats: Vec<&LiveChannel> = s.channels.iter().filter(|c| c.kind == LiveChannelKind::Category).collect();
    cats.sort_by(by_position);
    let is_category =
        |id: Option<crate::id::Id>| id.and_then(|i| s.channel(i)).is_some_and(|p| p.kind == LiveChannelKind::Category);

    let mut categories = Vec::new();
    for cat in &cats {
        let mut children: Vec<&LiveChannel> = s.channels.iter().filter(|c| c.parent_id == Some(cat.id)).collect();
        children.sort_by(by_position);
        let mut channels = Vec::new();
        for ch in children {
            match ch.kind {
                LiveChannelKind::Other(n) => notes.unmanaged.push(unmanaged_note(ch, n)),
                _ => channels.extend(channel_spec(s, ch, Some(cat))),
            }
        }
        categories.push(CategorySpec {
            id: Some(cat.id),
            name: cat.name.clone(),
            position: None,
            overwrites: Some(overwrites(s, &cat.overwrites)),
            channels,
        });
    }

    let mut top: Vec<&LiveChannel> =
        s.channels.iter().filter(|c| c.kind != LiveChannelKind::Category && !is_category(c.parent_id)).collect();
    top.sort_by(by_position);
    let mut channels = Vec::new();
    for ch in top {
        match ch.kind {
            LiveChannelKind::Other(n) => notes.unmanaged.push(unmanaged_note(ch, n)),
            _ => channels.extend(channel_spec(s, ch, None)),
        }
    }

    let spec = GuildSpec {
        guild: GuildInfo { id: Some(s.guild.id), name: Some(s.guild.name.clone()), manage_settings: false },
        roles: role_specs,
        categories,
        channels,
        prune: PruneSpec::default(),
    };
    (spec, notes)
}
