//! Day-to-day operations for agents (US4, FR-024): roles, channels, overwrites, members,
//! messages and moderation.
//!
//! Every write: refuses in read-only mode before any request, checks the bot's permissions
//! and role order (avoiding predictable 403s, research R9), asks for confirmation when
//! destructive (FR-018), records an audit-log reason (FR-019), and clears the snapshot cache
//! (research R18).

use std::str::FromStr;

use guildsmith_core::explain::{self, MemberInfo, StepKind, Who};
use guildsmith_core::snapshot::{LiveChannel, LiveChannelKind, LiveOverwrite, OverwriteKind, Snapshot};
use guildsmith_core::spec::{ChannelKind, Color};
use guildsmith_core::{Id, Permission, PermissionSet, suggest};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{ChannelFields, ChannelPosition, DiscordPort, Member, Message, RoleFields};
use crate::preflight::{self, PreflightProblem};
use crate::resolve::{self, Resolution};
use crate::services::apply::preflight_refusal;
use crate::services::guilds;

// ---- shared ----------------------------------------------------------------------------------

/// A create-or-update result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Upserted {
    pub id: Id,
    pub created: bool,
    /// `field old → new`, empty when nothing needed to change.
    pub changes: Vec<String>,
}

/// What a confirmed delete removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Deleted {
    pub deleted: String,
}

fn invalid(msg: impl Into<String>) -> ServiceError {
    ServiceError::Invalid(msg.into())
}

/// Turns a name resolution into an id, or a not-found / ambiguous error.
fn resolved(r: Resolution, what: &'static str, input: &str) -> Result<Id, ServiceError> {
    match r {
        Resolution::Resolved(id) => Ok(id),
        Resolution::Ambiguous(candidates) => Err(ServiceError::Ambiguous { what, input: input.to_owned(), candidates }),
        Resolution::NotFound { suggestions } => {
            Err(ServiceError::NotFound { what, input: input.to_owned(), suggestions })
        }
    }
}

/// The server id and a (possibly cached) snapshot.
async fn server<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<(Id, Snapshot), ServiceError> {
    let id = guilds::resolve(app, guild).await?;
    Ok((id, app.snapshot(id).await?))
}

/// Server for a write: read-only mode refuses before any request.
async fn writable<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<(Id, Snapshot), ServiceError> {
    app.policy.ensure_writable()?;
    server(app, guild).await
}

fn refuse_if(problems: &[PreflightProblem]) -> Result<(), ServiceError> {
    if problems.is_empty() { Ok(()) } else { Err(preflight_refusal(problems).into()) }
}

/// The bot's permissions for `tool`, plus role order for `role` when given.
fn require(s: &Snapshot, tool: &str, role: Option<Id>) -> Result<(), ServiceError> {
    let mut problems = preflight::check_tools(s, [tool]);
    if let Some(r) = role {
        problems.extend(preflight::check_role_edit(s, r));
    }
    refuse_if(&problems)
}

fn parse_perms(names: &[String]) -> Result<PermissionSet, ServiceError> {
    let mut bits = 0;
    for n in names {
        bits |= PermissionSet::parse_name(n).map_err(|_| {
            let hint = suggest::closest(n, Permission::ALL.iter().map(|p| p.name()), 1);
            let hint = hint.first().map(|s| format!("; did you mean `{s}`?")).unwrap_or_default();
            invalid(format!("unknown permission `{n}`{hint}"))
        })?;
    }
    Ok(PermissionSet::from_bits(bits))
}

fn role_name(s: &Snapshot, id: Id) -> String {
    if id == s.guild.id {
        "@everyone".to_owned()
    } else {
        s.role(id).map_or_else(|| format!("role:{id}"), |r| format!("@{}", r.name))
    }
}

fn change<T: std::fmt::Display>(field: &str, old: T, new: T) -> String {
    format!("{field} {old} → {new}")
}

// ---- refresh ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Refreshed {
    pub cleared: Vec<Id>,
}

/// Clears the cache for one server, or every server.
pub async fn refresh<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<Refreshed, ServiceError> {
    match guild.map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => {
            let id = guilds::resolve(app, Some(g)).await?;
            app.invalidate(id);
            Ok(Refreshed { cleared: vec![id] })
        }
        None => Ok(Refreshed { cleared: app.cache.clear() }),
    }
}

// ---- roles -----------------------------------------------------------------------------------

/// Role attributes from an agent. `None` leaves an attribute alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleInput {
    pub name: Option<String>,
    /// `#RRGGBB`.
    pub color: Option<String>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
    pub permissions: Option<Vec<String>>,
}

fn role_fields(input: &RoleInput) -> Result<RoleFields, ServiceError> {
    let color = input
        .color
        .as_deref()
        .map(|c| Color::from_str(c).map(|c| c.0).map_err(|e| invalid(e.to_string())))
        .transpose()?;
    Ok(RoleFields {
        name: input.name.clone(),
        color,
        hoist: input.hoist,
        mentionable: input.mentionable,
        permissions: input.permissions.as_deref().map(parse_perms).transpose()?,
    })
}

/// Creates a role (no `role`) or updates one. Only sends what differs.
pub async fn role_upsert<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    role: Option<&str>,
    input: &RoleInput,
    reason: Option<&str>,
) -> Result<Upserted, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let mut f = role_fields(input)?;
    let Some(target) = role else {
        let name = f.name.clone().ok_or_else(|| invalid("`name` is needed to create a role"))?;
        require(&s, "role_upsert", None)?;
        let id = app.port.create_role(gid, &f, &audit_reason(&format!("create role @{name}"), reason)).await?;
        app.invalidate(gid);
        return Ok(Upserted { id, created: true, changes: vec![format!("created @{name}")] });
    };
    let id = resolved(resolve::resolve_role(&s, target), "role", target)?;
    let live = s.role(id).expect("resolved role is live");
    let mut changes = Vec::new();
    if f.name.as_ref() == Some(&live.name) || id == s.guild.id {
        f.name = None;
    }
    if f.color == Some(live.color) {
        f.color = None;
    }
    if f.hoist == Some(live.hoist) {
        f.hoist = None;
    }
    if f.mentionable == Some(live.mentionable) {
        f.mentionable = None;
    }
    if f.permissions == Some(live.permissions) {
        f.permissions = None;
    }
    if let Some(v) = &f.name {
        changes.push(change("name", &live.name, v));
    }
    if let Some(v) = f.color {
        changes.push(change("color", format!("#{:06X}", live.color), format!("#{v:06X}")));
    }
    if let Some(v) = f.hoist {
        changes.push(change("hoist", live.hoist, v));
    }
    if let Some(v) = f.mentionable {
        changes.push(change("mentionable", live.mentionable, v));
    }
    if let Some(v) = f.permissions {
        changes.push(change("permissions", live.permissions.names().join(", "), v.names().join(", ")));
    }
    if changes.is_empty() {
        return Ok(Upserted { id, created: false, changes });
    }
    if live.managed && f != (RoleFields { permissions: f.permissions, ..RoleFields::default() }) {
        let problem =
            PreflightProblem::ManagedRoleEdit { role: role_name(&s, id), field: "anything but permissions".to_owned() };
        return Err(preflight_refusal(&[problem]).into());
    }
    require(&s, "role_upsert", (id != s.guild.id).then_some(id))?;
    let summary = format!("update role {}: {}", role_name(&s, id), changes.join(", "));
    app.port.update_role(gid, id, &f, &audit_reason(&summary, reason)).await?;
    app.invalidate(gid);
    Ok(Upserted { id, created: false, changes })
}

/// Deletes a role after confirmation. Never `@everyone` or a managed role.
pub async fn role_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    role: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let id = resolved(resolve::resolve_role(&s, role), "role", role)?;
    let live = s.role(id).expect("resolved role is live");
    if id == s.guild.id {
        return Err(invalid("@everyone can't be deleted"));
    }
    if live.managed {
        return Err(invalid(format!("@{} is managed by an integration or bot and can't be deleted", live.name)));
    }
    let what = format!("role @{} ({id})", live.name);
    let holders = format!("delete {what}; members lose it and its channel overwrites are removed");
    confirm::guarded(confirmed, vec![holders], || async {
        require(&s, "role_delete", Some(id))?;
        app.port.delete_role(gid, id, &audit_reason(&format!("delete {what}"), reason)).await?;
        app.invalidate(gid);
        Ok(Deleted { deleted: what.clone() })
    })
    .await
}

// ---- channels --------------------------------------------------------------------------------

/// Channel attributes from an agent. `None` leaves an attribute alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelInput {
    pub name: Option<String>,
    /// `text`, `announcement`, `voice`, `stage`, `forum`, `media` or `category`.
    pub kind: Option<String>,
    /// A category, or `""` / `none` for the top level.
    pub parent: Option<String>,
    pub topic: Option<String>,
    pub nsfw: Option<bool>,
    pub slowmode: Option<u32>,
    pub bitrate: Option<u32>,
    pub user_limit: Option<u32>,
    pub rtc_region: Option<String>,
}

fn parse_kind(k: &str) -> Result<LiveChannelKind, ServiceError> {
    if k == "category" {
        return Ok(LiveChannelKind::Category);
    }
    let kinds = [
        ChannelKind::Text,
        ChannelKind::Announcement,
        ChannelKind::Voice,
        ChannelKind::Stage,
        ChannelKind::Forum,
        ChannelKind::Media,
    ];
    kinds.into_iter().find(|c| c.as_str() == k).map(LiveChannelKind::Known).ok_or_else(|| {
        invalid(format!("unknown channel kind `{k}`; use text, announcement, voice, stage, forum, media or category"))
    })
}

/// `Some(None)` = top level; `None` = not given.
#[allow(clippy::option_option)] // three states: not given, top level, a category
fn parse_parent(s: &Snapshot, parent: Option<&str>) -> Result<Option<Option<Id>>, ServiceError> {
    let Some(p) = parent.map(str::trim) else { return Ok(None) };
    if p.is_empty() || p == "none" {
        return Ok(Some(None));
    }
    let id = resolved(resolve::resolve_channel(s, p), "category", p)?;
    if s.channel(id).is_some_and(|c| c.kind != LiveChannelKind::Category) {
        return Err(invalid(format!("`{p}` is a channel, not a category")));
    }
    Ok(Some(Some(id)))
}

fn channel_label(c: &LiveChannel) -> String {
    match c.kind {
        LiveChannelKind::Category => format!("category {}", c.name),
        LiveChannelKind::Known(k) if k.is_text_like() => format!("#{}", c.name),
        _ => c.name.clone(),
    }
}

/// The differences between `input` and a live channel, as fields to send.
#[allow(clippy::option_option)] // `parent` as `parse_parent` returns it
fn channel_changes(
    s: &Snapshot,
    c: &LiveChannel,
    input: &ChannelInput,
    parent: Option<Option<Id>>,
) -> Result<(ChannelFields, Vec<String>), ServiceError> {
    let mut f = ChannelFields::default();
    let mut changes = Vec::new();
    if let Some(n) = input.name.as_ref().filter(|n| **n != c.name) {
        changes.push(change("name", &c.name, n));
        f.name = Some(n.clone());
    }
    if let Some(k) = input.kind.as_deref().map(parse_kind).transpose()?.filter(|k| *k != c.kind) {
        let text_news =
            |k: LiveChannelKind| matches!(k, LiveChannelKind::Known(ChannelKind::Text | ChannelKind::Announcement));
        if !(text_news(k) && text_news(c.kind) && s.guild.features.contains("NEWS")) {
            return Err(invalid(
                "only text ↔ announcement can change in place (with the NEWS feature); delete and recreate the channel instead",
            ));
        }
        changes.push("kind changed".to_owned());
        f.kind = Some(k);
    }
    if let Some(p) = parent.filter(|p| *p != c.parent_id) {
        let name = |id: Option<Id>| id.and_then(|i| s.channel(i)).map_or_else(|| "none".to_owned(), |p| p.name.clone());
        changes.push(change("category", name(c.parent_id), name(p)));
        f.parent_id = Some(p);
    }
    if let Some(t) = input.topic.as_ref().filter(|t| t.as_str() != c.topic.as_deref().unwrap_or("")) {
        changes.push(change("topic", format!("{:?}", c.topic.as_deref().unwrap_or("")), format!("{t:?}")));
        f.topic = Some(t.clone());
    }
    if let Some(v) = input.nsfw.filter(|v| *v != c.nsfw) {
        changes.push(change("nsfw", c.nsfw, v));
        f.nsfw = Some(v);
    }
    if let Some(v) = input.slowmode.filter(|v| *v != c.slowmode) {
        changes.push(change("slowmode", c.slowmode, v));
        f.slowmode = Some(v);
    }
    if let Some(v) = input.bitrate.filter(|v| Some(*v) != c.bitrate) {
        changes.push(change("bitrate", c.bitrate.unwrap_or(0), v));
        f.bitrate = Some(v);
    }
    if let Some(v) = input.user_limit.filter(|v| Some(*v) != c.user_limit) {
        changes.push(change("user_limit", c.user_limit.unwrap_or(0), v));
        f.user_limit = Some(v);
    }
    if let Some(r) = &input.rtc_region {
        let want = (r != "auto" && !r.is_empty()).then(|| r.clone());
        if want != c.rtc_region {
            changes.push(change("rtc_region", c.rtc_region.as_deref().unwrap_or("auto"), r));
            f.rtc_region = Some(want);
        }
    }
    Ok((f, changes))
}

/// Creates a channel or category (no `channel`) or updates one. Only sends what differs.
pub async fn channel_upsert<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: Option<&str>,
    input: &ChannelInput,
    reason: Option<&str>,
) -> Result<Upserted, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let parent = parse_parent(&s, input.parent.as_deref())?;
    let Some(target) = channel else {
        let name = input.name.clone().ok_or_else(|| invalid("`name` is needed to create a channel"))?;
        let kind = parse_kind(input.kind.as_deref().unwrap_or("text"))?;
        if kind == LiveChannelKind::Category && parent.flatten().is_some() {
            return Err(invalid("a category can't be inside another category"));
        }
        require(&s, "channel_upsert", None)?;
        let f = ChannelFields {
            name: Some(name.clone()),
            kind: Some(kind),
            parent_id: parent,
            topic: input.topic.clone(),
            nsfw: input.nsfw,
            slowmode: input.slowmode,
            bitrate: input.bitrate,
            user_limit: input.user_limit,
            rtc_region: input.rtc_region.as_ref().map(|r| (r != "auto").then(|| r.clone())),
            ..ChannelFields::default()
        };
        let summary = format!("create {} {name}", input.kind.as_deref().unwrap_or("text"));
        let id = app.port.create_channel(gid, &f, &audit_reason(&summary, reason)).await?;
        app.invalidate(gid);
        return Ok(Upserted { id, created: true, changes: vec![format!("created {name}")] });
    };
    let id = resolved(resolve::resolve_channel(&s, target), "channel", target)?;
    let live = s.channel(id).expect("resolved channel is live");
    let (f, changes) = channel_changes(&s, live, input, parent)?;
    if changes.is_empty() {
        return Ok(Upserted { id, created: false, changes });
    }
    require(&s, "channel_upsert", None)?;
    let summary = format!("update {}: {}", channel_label(live), changes.join(", "));
    app.port.update_channel(gid, id, &f, &audit_reason(&summary, reason)).await?;
    app.invalidate(gid);
    Ok(Upserted { id, created: false, changes })
}

/// Deletes a channel or category after confirmation.
pub async fn channel_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let id = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let live = s.channel(id).expect("resolved channel is live");
    let what = format!("{} ({id})", channel_label(live));
    let mut would = vec![format!("delete {what} and its message history")];
    let children = s.channels.iter().filter(|c| c.parent_id == Some(id)).count();
    if children > 0 {
        would.push(format!("its {children} channel(s) move to the top level"));
    }
    confirm::guarded(confirmed, would, || async {
        require(&s, "channel_delete", None)?;
        app.port.delete_channel(gid, id, &audit_reason(&format!("delete {what}"), reason)).await?;
        app.invalidate(gid);
        Ok(Deleted { deleted: what.clone() })
    })
    .await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Moved {
    pub moved: usize,
}

/// Puts channels in the given order (first = top) in one positions request. With `parent`,
/// channels elsewhere are first moved into it, each in its own update (research R8).
pub async fn channels_reorder<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    order: &[String],
    parent: Option<&str>,
    reason: Option<&str>,
) -> Result<Moved, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    if order.is_empty() {
        return Err(invalid("`order` needs at least one channel"));
    }
    let parent = parse_parent(&s, parent)?;
    let ids = order
        .iter()
        .map(|c| resolved(resolve::resolve_channel(&s, c), "channel", c))
        .collect::<Result<Vec<Id>, _>>()?;
    require(&s, "channels_reorder", None)?;
    let mut moves = 0;
    if let Some(p) = parent {
        for id in &ids {
            let c = s.channel(*id).expect("resolved");
            if c.parent_id != p {
                let f = ChannelFields { parent_id: Some(p), ..ChannelFields::default() };
                let summary = format!("move {} for reorder", channel_label(c));
                app.port.update_channel(gid, *id, &f, &audit_reason(&summary, reason)).await?;
                moves += 1;
            }
        }
    }
    let view = if moves > 0 { app.port.snapshot(gid).await? } else { s };
    let wanted: Vec<(Id, Option<i32>)> = ids.iter().map(|id| (*id, None)).collect();
    let positions: Vec<ChannelPosition> = guildsmith_core::planner::positions::channel_positions(&view, &wanted)
        .into_iter()
        .map(|(id, position)| ChannelPosition { id, position, parent_id: None })
        .collect();
    if !positions.is_empty() {
        let summary = format!("reorder channels ({})", ids.len());
        app.port.reorder_channels(gid, &positions, &audit_reason(&summary, reason)).await?;
    }
    if moves > 0 || !positions.is_empty() {
        app.invalidate(gid);
    }
    Ok(Moved { moved: positions.len().max(moves) })
}

// ---- permission overwrites ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OverwriteView {
    pub target: String,
    #[schemars(with = "Vec<String>")]
    pub allow: PermissionSet,
    #[schemars(with = "Vec<String>")]
    pub deny: PermissionSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OverwriteSet {
    /// `null` when the overwrite was cleared.
    pub overwrite: Option<OverwriteView>,
}

/// A role or a member, for overwrites and explanations.
enum Subject {
    Role(Id),
    Member(Member),
}

/// Resolves `input` as a role first, then as a member.
async fn subject<P: DiscordPort>(app: &App<P>, gid: Id, s: &Snapshot, input: &str) -> Result<Subject, ServiceError> {
    let raw = input.trim();
    let member_form = raw.starts_with("member:") || (raw.starts_with("<@") && !raw.starts_with("<@&"));
    if !member_form {
        match resolve::resolve_role(s, raw) {
            Resolution::Resolved(id) => return Ok(Subject::Role(id)),
            amb @ Resolution::Ambiguous(_) => return resolved(amb, "role", raw).map(Subject::Role),
            Resolution::NotFound { .. } => {}
        }
    }
    let m = find_member(app, gid, raw.strip_prefix("member:").unwrap_or(raw)).await?;
    Ok(Subject::Member(m))
}

/// Sets, replaces or clears one overwrite, sending the channel's full list (research R7).
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn permissions_set<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    target: &str,
    allow: &[String],
    deny: &[String],
    clear: bool,
    reason: Option<&str>,
) -> Result<OverwriteSet, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let live = s.channel(cid).expect("resolved channel is live");
    let (allow, deny) = (parse_perms(allow)?, parse_perms(deny)?);
    if !allow.intersection(deny).is_empty() {
        return Err(invalid(format!("{} is in both allow and deny", allow.intersection(deny).names().join(", "))));
    }
    let (kind, tid, label) = match subject(app, gid, &s, target).await? {
        Subject::Role(id) => (OverwriteKind::Role, id, role_name(&s, id)),
        Subject::Member(m) => (OverwriteKind::Member, m.id, format!("member:{} ({})", m.id, m.username)),
    };
    let current = live.overwrites.iter().find(|o| o.kind == kind && o.id == tid);
    let mut list: Vec<LiveOverwrite> =
        live.overwrites.iter().filter(|o| !(o.kind == kind && o.id == tid)).cloned().collect();
    let wanted = (!clear).then_some(LiveOverwrite { id: tid, kind, allow, deny });
    if current == wanted.as_ref() {
        let overwrite = wanted.map(|o| OverwriteView { target: label, allow: o.allow, deny: o.deny });
        return Ok(OverwriteSet { overwrite });
    }
    list.extend(wanted.clone());
    let mut problems = preflight::check_tools(&s, ["permissions_set"]);
    if !s.bot.permissions.contains(Permission::Administrator) && s.bot.user_id != s.guild.owner_id {
        let me = MemberInfo { id: s.bot.user_id, role_ids: s.bot.role_ids.clone(), timed_out: false };
        let have = explain::explain(&s, cid, &Who::Member(me), None).map_or(s.bot.permissions, |e| e.allowed);
        for p in allow.union(deny).iter().filter(|p| !have.contains(*p) || *p == Permission::ManageRoles) {
            problems.push(PreflightProblem::CannotGrant { perm: p.name().to_owned(), channel: live.name.clone() });
        }
    }
    refuse_if(&problems)?;
    let f = ChannelFields { overwrites: Some(list), ..ChannelFields::default() };
    let verb = if clear { "clear" } else { "set" };
    let summary = format!("{verb} overwrite for {label} on {}", channel_label(live));
    app.port.update_channel(gid, cid, &f, &audit_reason(&summary, reason)).await?;
    app.invalidate(gid);
    Ok(OverwriteSet { overwrite: wanted.map(|o| OverwriteView { target: label, allow: o.allow, deny: o.deny }) })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ExplainStep {
    pub step: StepKind,
    pub source: String,
    /// e.g. `+view_channel −send_messages`.
    pub effect: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Explained {
    #[schemars(with = "Vec<String>")]
    pub allowed: PermissionSet,
    #[schemars(with = "Vec<String>")]
    pub denied: PermissionSet,
    pub steps: Vec<ExplainStep>,
    /// For a queried permission: whether it's allowed and which step decided it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

/// Why a member or role can or can't do things in a channel (FR-030, research R13).
pub async fn permissions_explain<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    who: &str,
    permission: Option<&str>,
) -> Result<Explained, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let cid = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let query = permission
        .map(|p| Permission::from_name(p).ok_or_else(|| invalid(format!("unknown permission `{p}`"))))
        .transpose()?;
    let subject = match subject(app, gid, &s, who).await? {
        Subject::Role(id) => Who::Role(id),
        Subject::Member(m) => Who::Member(MemberInfo { id: m.id, role_ids: m.role_ids, timed_out: m.timed_out }),
    };
    let e = explain::explain(&s, cid, &subject, query).map_err(|e| invalid(e.to_string()))?;
    let steps: Vec<ExplainStep> = e
        .steps
        .iter()
        .map(|st| {
            let mut parts: Vec<String> = st.added.names().into_iter().map(|n| format!("+{n}")).collect();
            parts.extend(st.removed.names().into_iter().map(|n| format!("−{n}")));
            ExplainStep { step: st.step, source: st.source.clone(), effect: parts.join(" ") }
        })
        .collect();
    let verdict = query.map(|q| {
        let state = if e.allowed.contains(q) { "allowed" } else { "denied" };
        match e.decided_by.and_then(|i| e.steps.get(i)) {
            Some(st) => format!("{q} is {state}; decided by {}", st.source),
            None => format!("{q} is {state}; no role or overwrite grants it"),
        }
    });
    Ok(Explained { allowed: e.allowed, denied: e.denied, steps, verdict })
}

// ---- members ---------------------------------------------------------------------------------

/// One member. `name` is user-chosen text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MemberRow {
    pub id: Id,
    pub name: String,
    pub roles: Vec<String>,
}

fn member_row(s: &Snapshot, m: &Member) -> MemberRow {
    let name = m.nick.clone().or_else(|| m.display_name.clone()).unwrap_or_else(|| m.username.clone());
    MemberRow { id: m.id, name, roles: m.role_ids.iter().map(|r| role_name(s, *r)).collect() }
}

/// Finds a member by mention, id, username, display name or nickname.
async fn find_member<P: DiscordPort>(app: &App<P>, gid: Id, input: &str) -> Result<Member, ServiceError> {
    let raw = input.trim();
    let bare = raw.trim_start_matches("<@!").trim_start_matches("<@").trim_end_matches('>');
    if let Ok(id) = bare.parse::<Id>() {
        return Ok(app.port.member(gid, id).await?);
    }
    let name = raw.strip_prefix('@').unwrap_or(raw);
    let found = app.port.search_members(gid, name, 25).await?;
    let id = resolved(resolve::resolve_member(&found, raw), "member", raw)?;
    Ok(found.into_iter().find(|m| m.id == id).expect("resolved member was found"))
}

/// Searches members by the start of their username or nickname.
pub async fn members_find<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    query: &str,
    limit: u16,
) -> Result<Vec<MemberRow>, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let found = app.port.search_members(gid, query.trim(), limit.clamp(1, 1000)).await?;
    Ok(found.iter().map(|m| member_row(&s, m)).collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MemberRoles {
    pub member: Id,
    pub roles: Vec<String>,
}

/// Adds and removes roles on a member; only roles that change are sent.
pub async fn member_roles<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    member: &str,
    add: &[String],
    remove: &[String],
    reason: Option<&str>,
) -> Result<MemberRoles, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let m = find_member(app, gid, member).await?;
    let ids = |list: &[String]| {
        list.iter().map(|r| resolved(resolve::resolve_role(&s, r), "role", r)).collect::<Result<Vec<Id>, _>>()
    };
    let (add, remove) = (ids(add)?, ids(remove)?);
    let mut roles = m.role_ids.clone();
    let todo: Vec<(Id, bool)> = add
        .iter()
        .filter(|r| !m.role_ids.contains(r))
        .map(|r| (*r, true))
        .chain(remove.iter().filter(|r| m.role_ids.contains(r)).map(|r| (*r, false)))
        .collect();
    let mut problems = if todo.is_empty() { Vec::new() } else { preflight::check_tools(&s, ["member_roles"]) };
    for (r, _) in &todo {
        problems.extend(preflight::check_role_edit(&s, *r));
        if *r == s.guild.id || s.role(*r).is_some_and(|l| l.managed) {
            return Err(invalid(format!("{} can't be given or taken away", role_name(&s, *r))));
        }
    }
    refuse_if(&problems)?;
    for (r, adding) in todo {
        let who = format!("{} ({})", m.username, m.id);
        if adding {
            let summary = format!("give {} to {who}", role_name(&s, r));
            app.port.add_member_role(gid, m.id, r, &audit_reason(&summary, reason)).await?;
            roles.push(r);
        } else {
            let summary = format!("take {} from {who}", role_name(&s, r));
            app.port.remove_member_role(gid, m.id, r, &audit_reason(&summary, reason)).await?;
            roles.retain(|x| *x != r);
        }
    }
    Ok(MemberRoles { member: m.id, roles: roles.iter().map(|r| role_name(&s, *r)).collect() })
}

// ---- messages --------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Sent {
    pub id: Id,
    pub url: String,
}

/// A message as tools return it. `author` and `content` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MessageRow {
    pub id: Id,
    pub author: String,
    pub content: String,
    pub ts: String,
}

pub const READ_DEFAULT: u8 = 20;
pub const READ_MAX: u8 = 100;

fn text_channel(s: &Snapshot, channel: &str) -> Result<Id, ServiceError> {
    let id = resolved(resolve::resolve_channel(s, channel), "channel", channel)?;
    match s.channel(id).map(|c| c.kind) {
        Some(LiveChannelKind::Known(k)) if !k.is_forum_like() => Ok(id),
        _ => Err(invalid(format!("`{channel}` is not a channel that holds messages"))),
    }
}

/// Posts a message as the bot.
pub async fn message_send<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    content: &str,
) -> Result<Sent, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = text_channel(&s, channel)?;
    if content.trim().is_empty() || content.chars().count() > 2000 {
        return Err(invalid("content must be 1–2000 characters"));
    }
    require(&s, "message_send", None)?;
    let m = app.port.send_message(cid, content).await?;
    Ok(Sent { id: m.id, url: format!("https://discord.com/channels/{gid}/{cid}/{}", m.id) })
}

/// Reads recent messages, newest first: limit default 20, max 100.
pub async fn messages_read<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    limit: Option<u8>,
    before: Option<Id>,
) -> Result<Vec<Message>, ServiceError> {
    let (_, s) = server(app, guild).await?;
    let cid = text_channel(&s, channel)?;
    let limit = limit.unwrap_or(READ_DEFAULT).clamp(1, READ_MAX);
    Ok(app.port.read_messages(cid, limit, before).await?)
}

/// The message rows for structured output.
#[must_use]
pub fn message_rows(messages: &[Message]) -> Vec<MessageRow> {
    messages
        .iter()
        .map(|m| MessageRow {
            id: m.id,
            author: m.author_name.clone(),
            content: m.content.clone(),
            ts: m.timestamp.clone(),
        })
        .collect()
}

// ---- moderation ------------------------------------------------------------------------------

/// The longest timeout Discord allows.
pub const MAX_TIMEOUT_SECS: u64 = 28 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TimedOut {
    pub member: Id,
    /// RFC 3339, or `null` when the timeout was cleared.
    pub until: Option<String>,
}

/// Times a member out for `seconds` (≤ 28 days); `0` clears it.
pub async fn member_timeout<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    member: &str,
    seconds: u64,
    reason: Option<&str>,
) -> Result<TimedOut, ServiceError> {
    if seconds > MAX_TIMEOUT_SECS {
        return Err(invalid("a timeout can be at most 28 days (2419200 seconds)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let m = find_member(app, gid, member).await?;
    require(&s, "member_timeout", None)?;
    let until = (seconds > 0).then(|| date::rfc3339_after(app.clock.as_ref(), seconds));
    let summary = match &until {
        Some(t) => format!("time out {} until {t}", m.username),
        None => format!("clear timeout for {}", m.username),
    };
    app.port.timeout(gid, m.id, until.as_deref(), &audit_reason(&summary, reason)).await?;
    Ok(TimedOut { member: m.id, until })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Removed {
    pub member: Id,
    pub action: String,
}

/// Kicks a member after confirmation.
pub async fn member_kick<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    member: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Removed>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let m = find_member(app, gid, member).await?;
    let who = format!("{} ({})", m.username, m.id);
    confirm::guarded(confirmed, vec![format!("kick {who}; they can rejoin with an invite")], || async {
        require(&s, "member_kick", None)?;
        app.port.kick(gid, m.id, &audit_reason(&format!("kick {who}"), reason)).await?;
        Ok(Removed { member: m.id, action: "kicked".to_owned() })
    })
    .await
}

/// Bans a user after confirmation, optionally deleting their recent messages (≤ 7 days).
pub async fn member_ban<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    member: &str,
    delete_message_seconds: u32,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Removed>, ServiceError> {
    if delete_message_seconds > 604_800 {
        return Err(invalid("delete_message_seconds can be at most 604800 (7 days)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let m = find_member(app, gid, member).await?;
    let who = format!("{} ({})", m.username, m.id);
    let mut would = vec![format!("ban {who} from the server")];
    if delete_message_seconds > 0 {
        would.push(format!("delete their messages from the last {delete_message_seconds} seconds"));
    }
    confirm::guarded(confirmed, would, || async {
        require(&s, "member_ban", None)?;
        app.port.ban(gid, m.id, delete_message_seconds, &audit_reason(&format!("ban {who}"), reason)).await?;
        Ok(Removed { member: m.id, action: "banned".to_owned() })
    })
    .await
}

/// Lifts a ban.
pub async fn ban_remove<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    user: &str,
    reason: Option<&str>,
) -> Result<Removed, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let id: Id =
        user.trim().trim_start_matches("<@!").trim_start_matches("<@").trim_end_matches('>').parse().map_err(|_| {
            invalid("`user` must be a user id (banned users aren't members, so names can't be looked up)")
        })?;
    require(&s, "ban_remove", None)?;
    app.port.unban(gid, id, &audit_reason(&format!("unban {id}"), reason)).await?;
    Ok(Removed { member: id, action: "unbanned".to_owned() })
}
