//! `diff(spec, snapshot) -> Plan`: field-level comparison of the desired and live structure
//! (FR-007). Attributes the spec leaves out are not managed and never produce a change.

use std::collections::BTreeMap;

use crate::hash;
use crate::live::LiveExtras;
use crate::matching::{self, PlanError, stored_channel_name};
use crate::permissions::PermissionSet;
use crate::planner::SpecAssets;
use crate::planner::{self, Builder, ChannelAttrs, FieldChange, OpKind, OverwriteOp, Plan, Ref, Risk, RoleAttrs};
use crate::snapshot::{LiveChannel, LiveOverwrite, LiveRole, OverwriteKind, Snapshot};
use crate::spec::{ChannelSpec, ForumTag, GuildSpec, RoleSpec};
use crate::validate::{self, Severity};

fn color_text(c: u32) -> String {
    if c == 0 { "none".to_owned() } else { format!("#{c:06X}") }
}

fn perms_text(p: PermissionSet) -> String {
    let names = p.names();
    if names.is_empty() { "none".to_owned() } else { names.join(", ") }
}

fn quoted(s: &str) -> String {
    const MAX: usize = 40;
    if s.chars().count() > MAX {
        format!("\"{}…\"", s.chars().take(MAX).collect::<String>())
    } else {
        format!("\"{s}\"")
    }
}

fn change(field: &str, old: String, new: String) -> FieldChange {
    FieldChange { field: field.to_owned(), old, new }
}

/// Field changes for a matched role, plus the attributes to send.
#[must_use]
pub fn role_changes(r: &RoleSpec, live: &LiveRole, everyone: bool) -> (Vec<FieldChange>, RoleAttrs) {
    let mut changes = Vec::new();
    let mut attrs = RoleAttrs::default();
    if !everyone && r.name != live.name {
        changes.push(change("name", quoted(&live.name), quoted(&r.name)));
        attrs.name = Some(r.name.clone());
    }
    if let Some(c) = r.color
        && c.0 != live.color
    {
        changes.push(change("color", color_text(live.color), color_text(c.0)));
        attrs.color = Some(c.0);
    }
    if let Some(h) = r.hoist
        && h != live.hoist
    {
        changes.push(change("hoist", live.hoist.to_string(), h.to_string()));
        attrs.hoist = Some(h);
    }
    if let Some(m) = r.mentionable
        && m != live.mentionable
    {
        changes.push(change("mentionable", live.mentionable.to_string(), m.to_string()));
        attrs.mentionable = Some(m);
    }
    if let Some(p) = r.permissions
        && p != live.permissions
    {
        changes.push(change("permissions", perms_text(live.permissions), perms_text(p)));
        attrs.permissions = Some(p);
    }
    (changes, attrs)
}

/// Attributes for a new role.
#[must_use]
pub fn role_create_attrs(r: &RoleSpec) -> RoleAttrs {
    RoleAttrs {
        name: Some(r.name.clone()),
        color: r.color.map(|c| c.0),
        hoist: r.hoist,
        mentionable: r.mentionable,
        permissions: r.permissions,
    }
}

/// `"auto"` (or empty) means Discord picks the region.
fn region(r: &str) -> Option<String> {
    (!r.is_empty() && r != "auto").then(|| r.to_owned())
}

fn reaction(r: &str) -> Option<String> {
    (!r.is_empty()).then(|| r.to_owned())
}

/// Spec tags with the ids of the live tags they match (by id, then by name).
fn tags_with_ids(spec: &[ForumTag], live: &LiveChannel) -> Vec<ForumTag> {
    spec.iter()
        .map(|t| {
            let id = t.id.or_else(|| live.tags.iter().find(|l| l.name == t.name).map(|l| l.id));
            ForumTag { id, name: t.name.clone(), emoji: t.emoji.clone(), moderated: Some(t.moderated.unwrap_or(false)) }
        })
        .collect()
}

fn same_tags(spec: &[ForumTag], live: &LiveChannel) -> bool {
    let mut a: Vec<_> = spec.iter().map(|t| (t.name.clone(), t.emoji.clone(), t.moderated.unwrap_or(false))).collect();
    let mut b: Vec<_> = live.tags.iter().map(|t| (t.name.clone(), t.emoji.clone(), t.moderated)).collect();
    a.sort();
    b.sort();
    a == b
}

fn tag_names(tags: impl Iterator<Item = String>) -> String {
    let v: Vec<String> = tags.collect();
    if v.is_empty() { "none".to_owned() } else { v.join(", ") }
}

fn opt_text<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "none".to_owned(), |v| v.to_string())
}

/// Field changes for a matched channel (not its kind, parent or overwrites), plus the
/// attributes to send.
#[must_use]
pub fn channel_changes(ch: &ChannelSpec, live: &LiveChannel) -> (Vec<FieldChange>, ChannelAttrs) {
    let mut changes = Vec::new();
    let mut a = ChannelAttrs::default();
    let name = stored_channel_name(ch);
    if name != live.name {
        changes.push(change("name", quoted(&live.name), quoted(&name)));
        a.name = Some(name);
    }
    if let Some(t) = &ch.topic
        && t.as_str() != live.topic.as_deref().unwrap_or("")
    {
        changes.push(change("topic", quoted(live.topic.as_deref().unwrap_or("")), quoted(t)));
        a.topic = Some(t.clone());
    }
    if let Some(v) = ch.nsfw
        && v != live.nsfw
    {
        changes.push(change("nsfw", live.nsfw.to_string(), v.to_string()));
        a.nsfw = Some(v);
    }
    if let Some(v) = ch.slowmode
        && v != live.slowmode
    {
        changes.push(change("slowmode", live.slowmode.to_string(), v.to_string()));
        a.slowmode = Some(v);
    }
    if let Some(v) = ch.bitrate
        && Some(v) != live.bitrate
    {
        changes.push(change("bitrate", opt_text(live.bitrate), v.to_string()));
        a.bitrate = Some(v);
    }
    if let Some(v) = ch.user_limit
        && Some(v) != live.user_limit
    {
        changes.push(change("user_limit", opt_text(live.user_limit), v.to_string()));
        a.user_limit = Some(v);
    }
    if let Some(r) = &ch.rtc_region
        && region(r) != live.rtc_region
    {
        changes.push(change("rtc_region", live.rtc_region.clone().unwrap_or_else(|| "auto".into()), r.clone()));
        a.rtc_region = Some(region(r));
    }
    if let Some(tags) = &ch.tags
        && !same_tags(tags, live)
    {
        let old = tag_names(live.tags.iter().map(|t| t.name.clone()));
        changes.push(change("tags", old, tag_names(tags.iter().map(|t| t.name.clone()))));
        a.tags = Some(tags_with_ids(tags, live));
    }
    if let Some(r) = &ch.default_reaction
        && reaction(r) != live.default_reaction
    {
        changes.push(change("default_reaction", opt_text(live.default_reaction.as_ref()), opt_text(reaction(r))));
        a.default_reaction = Some(reaction(r));
    }
    (changes, a)
}

/// Attributes for a new channel.
#[must_use]
pub fn channel_create_attrs(ch: &ChannelSpec) -> ChannelAttrs {
    ChannelAttrs {
        name: Some(stored_channel_name(ch)),
        kind: Some(crate::snapshot::LiveChannelKind::Known(ch.kind)),
        topic: ch.topic.clone().filter(|t| !t.is_empty()),
        nsfw: ch.nsfw,
        slowmode: ch.slowmode,
        bitrate: ch.bitrate,
        user_limit: ch.user_limit,
        rtc_region: ch.rtc_region.as_deref().map(region),
        tags: ch.tags.as_ref().map(|t| {
            t.iter()
                .map(|t| ForumTag { id: None, moderated: Some(t.moderated.unwrap_or(false)), ..t.clone() })
                .collect()
        }),
        default_reaction: ch.default_reaction.as_deref().map(reaction),
    }
}

/// Whether the desired overwrites equal the live ones. Any target not created yet differs.
#[must_use]
pub fn same_overwrites(desired: &[OverwriteOp], live: &[LiveOverwrite]) -> bool {
    let mut want = Vec::with_capacity(desired.len());
    for o in desired {
        match &o.target {
            Ref::Id(id) => want.push((o.kind, *id, o.allow, o.deny)),
            Ref::New(_) => return false,
        }
    }
    let mut have: Vec<_> = live.iter().map(|o| (o.kind, o.id, o.allow, o.deny)).collect();
    want.sort();
    have.sort();
    want == have
}

/// `@everyone −view_channel; @Moderator +view_channel`.
pub fn describe_overwrites(
    s: &Snapshot,
    list: impl Iterator<Item = (OverwriteKind, Ref, PermissionSet, PermissionSet)>,
) -> String {
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    for (kind, target, allow, deny) in list {
        let who = match (kind, &target) {
            (OverwriteKind::Member, t) => format!("member:{t}"),
            (OverwriteKind::Role, Ref::Id(id)) if *id == s.guild.id => "@everyone".to_owned(),
            (OverwriteKind::Role, Ref::Id(id)) => {
                s.role(*id).map_or_else(|| format!("role:{id}"), |r| format!("@{}", r.name))
            }
            (OverwriteKind::Role, Ref::New(key)) => format!("@{}", key.trim_start_matches("role:")),
        };
        let mut perms: Vec<String> = allow.names().into_iter().map(|n| format!("+{n}")).collect();
        perms.extend(deny.names().into_iter().map(|n| format!("−{n}")));
        parts.insert(who, if perms.is_empty() { "(empty)".to_owned() } else { perms.join(" ") });
    }
    if parts.is_empty() {
        return "none".to_owned();
    }
    parts.into_iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("; ")
}

/// Compares a spec with a live snapshot and returns the ordered plan, or every problem that
/// stops it from being planned. Kinds beyond roles and channels are left alone: see
/// [`diff_with`].
pub fn diff(spec: &GuildSpec, snapshot: &Snapshot) -> Result<Plan, PlanError> {
    diff_with(spec, snapshot, &LiveExtras::default(), &SpecAssets::new())
}

/// [`diff`], plus every kind whose live state is in `extras` (US9). `assets` holds the blake3
/// of each file the spec names, by spec path.
pub fn diff_with(
    spec: &GuildSpec,
    snapshot: &Snapshot,
    extras: &LiveExtras,
    assets: &SpecAssets,
) -> Result<Plan, PlanError> {
    let invalid: Vec<String> = validate::validate(spec)
        .into_iter()
        .filter(|(sev, ..)| *sev == Severity::Error)
        .map(|(_, path, msg, _)| if path.0.is_empty() { msg } else { format!("{path}: {msg}") })
        .collect();
    if !invalid.is_empty() {
        return Err(PlanError { problems: invalid });
    }
    let m = matching::match_spec(spec, snapshot)?;
    let mut b = Builder::new(spec, snapshot, &m);
    b.roles();
    b.reorder_roles();
    b.categories();
    b.channels();
    b.reorder_channels();
    b.prune();
    b.kinds(extras, assets);
    if !b.errors.is_empty() {
        return Err(PlanError { problems: b.errors });
    }
    let ops = planner::order::order(b.ops);
    let mut warnings = b.warnings;
    let creates = ops.iter().filter(|o| matches!(o.kind, OpKind::CreateRole { .. })).count();
    if creates > planner::ROLE_CREATE_WARNING {
        warnings.push(format!(
            "this plan creates {creates} roles; Discord reportedly limits role creation (about 250 per 48 hours), \
             so consider applying in smaller batches"
        ));
    }
    let reorders =
        ops.iter().filter(|o| matches!(o.kind, OpKind::ReorderRoles { .. } | OpKind::ReorderChannels { .. }));
    let reorder_reads = u32::try_from(reorders.count()).unwrap_or(u32::MAX).saturating_mul(planner::SNAPSHOT_READS);
    let estimated_requests = u32::try_from(ops.len())
        .unwrap_or(u32::MAX)
        .saturating_add(planner::SNAPSHOT_READS)
        .saturating_add(reorder_reads);
    let risk = if ops.iter().any(|o| o.risk == Risk::Destructive) { Risk::Destructive } else { Risk::Safe };
    // v1 plans keep their hashes: the extras count only when something was read.
    let fingerprint = if extras.is_empty() {
        snapshot.fingerprint()
    } else {
        blake3::hash(format!("{}{}", snapshot.fingerprint(), extras.fingerprint()).as_bytes()).to_hex().to_string()
    };
    Ok(Plan {
        hash: hash::plan_hash(&ops, &fingerprint, spec),
        ops,
        unmanaged: b.unmanaged,
        warnings,
        estimated_requests,
        risk,
        snapshot_fingerprint: fingerprint,
        write_ids: b.write_ids,
    })
}
