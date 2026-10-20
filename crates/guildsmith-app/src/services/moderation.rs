//! The `moderation` group (US1, FR-011–016): bans (single and bulk), unbans, the ban list,
//! timeouts, kicks, prune, message purge, bulk role assignment, the audit log and raid
//! lockdown.
//!
//! Every write refuses in read-only mode before any request and checks the bot's permissions
//! first. Destructive actions preview their exact targets until confirmed. Bulk actions skip
//! the owner, the bot itself and members at or above the bot's highest role, with a reason,
//! and report every target as succeeded, failed or not attempted (SC-005).

use std::collections::BTreeSet;

use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::{Id, Permission};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::bulk::{
    BAN_BATCH, BatchResult, BulkAction, BulkOutcome, BulkPreview, BulkState, DELETE_BATCH, Failure, Hooks, Problem,
    SINGLE,
};
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, Incidents, Member, Message, PortError};
use crate::preflight::{self, PreflightProblem};
use crate::resolve;
use crate::services::live::{find_member, invalid, refuse_if, resolved, role_name, server, text_channel, writable};

/// The longest timeout Discord allows: 28 days.
pub const MAX_TIMEOUT_SECS: u64 = 28 * 86_400;
/// Bulk ban and ban-with-delete delete at most 7 days of messages.
pub const MAX_DELETE_SECS: u32 = 604_800;
/// Bulk delete only takes messages newer than 14 days.
pub const BULK_DELETE_MAX_AGE: i64 = 14 * 86_400;
/// A purge looks at most this many messages.
pub const PURGE_MAX: usize = 1000;
/// Raid protections last at most 24 hours.
pub const MAX_LOCKDOWN_SECS: u64 = 86_400;

// ---- shared ----------------------------------------------------------------------------------

/// A member or user an action targets. `name` is user-chosen text: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Target {
    pub id: Id,
    pub name: String,
}

/// A bulk tool's result: the preview, or what happened to every target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum BulkResult<T> {
    Preview(BulkPreview<T>),
    Done(BulkOutcome<T>),
}

impl<T> BulkResult<T> {
    #[must_use]
    pub fn is_preview(&self) -> bool {
        matches!(self, BulkResult::Preview(_))
    }
}

fn target(m: &Member) -> Target {
    let name = m.nick.clone().or_else(|| m.display_name.clone()).unwrap_or_else(|| m.username.clone());
    Target { id: m.id, name }
}

/// The ids in mention or plain form, or `None` for a name.
fn as_id(input: &str) -> Option<Id> {
    input.trim().trim_start_matches("<@!").trim_start_matches("<@").trim_end_matches('>').parse().ok()
}

/// Resolves member inputs, once each. With `users_ok`, an id that isn't a member is still a
/// target (e.g. banning someone who already left).
async fn targets<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    inputs: &[String],
    users_ok: bool,
) -> Result<Vec<(Target, Option<Member>)>, ServiceError> {
    if inputs.is_empty() {
        return Err(invalid("give at least one member"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for input in inputs {
        let found = if let (Some(id), true) = (as_id(input), users_ok) {
            match app.port.member(gid, id).await {
                Ok(m) => (target(&m), Some(m)),
                Err(PortError::NotFound(_)) => (Target { id, name: format!("user {id}") }, None),
                Err(e) => return Err(e.into()),
            }
        } else {
            let m = find_member(app, gid, input).await?;
            (target(&m), Some(m))
        };
        if seen.insert(found.0.id) {
            out.push(found);
        }
    }
    Ok(out)
}

fn top_position(s: &Snapshot, m: &Member) -> i64 {
    m.role_ids.iter().filter_map(|r| s.role(*r)).map(|r| r.position).max().unwrap_or(0)
}

/// Why the bot can't act on a member: the owner, itself, or someone at or above its role.
fn hierarchy_problem(s: &Snapshot, t: &Target, m: Option<&Member>) -> Option<Problem> {
    if t.id == s.guild.owner_id {
        return Some(Problem::Owner);
    }
    if t.id == s.bot.user_id {
        return Some(Problem::IsSelf);
    }
    let bot_is_owner = s.bot.user_id == s.guild.owner_id;
    m.filter(|m| !bot_is_owner && top_position(s, m) >= s.bot.top_role_position).map(|_| Problem::AboveBot)
}

fn action_for(name: &str, list: &[(Target, Option<Member>)], s: &Snapshot, batch: usize) -> BulkAction<Target> {
    let mut action = BulkAction::new(name, list.iter().map(|(t, _)| t.clone()).collect(), batch);
    for (t, m) in list {
        if let Some(p) = hierarchy_problem(s, t, m.as_ref()) {
            action = action.with_problem(t.clone(), p);
        }
    }
    action
}

/// Per-item errors keep a bulk action going; losing the token, the network or Discord itself
/// stops it.
fn is_fatal(e: &PortError) -> bool {
    matches!(e, PortError::Unauthorized | PortError::NoToken | PortError::NotBotToken | PortError::Network(_))
        || matches!(e, PortError::Discord { status, .. } if *status >= 500)
}

/// One item's result, or a stop.
fn one<T>(item: T, res: Result<(), PortError>) -> BatchResult<T> {
    match res {
        Err(e) if is_fatal(&e) => BatchResult::Fatal(e.to_string()),
        other => BatchResult::single(item, other.map_err(|e| e.to_string())),
    }
}

fn who(t: &Target) -> String {
    format!("{} ({})", t.name, t.id)
}

// ---- bans ------------------------------------------------------------------------------------

/// Bans members (or user ids) after confirmation, 200 per request when there is more than one,
/// optionally deleting their messages from the last `delete_message_seconds` (≤ 7 days).
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn member_ban<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    members: &[String],
    delete_message_seconds: u32,
    confirmed: bool,
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkResult<Target>, ServiceError> {
    if delete_message_seconds > MAX_DELETE_SECS {
        return Err(invalid("delete_message_seconds can be at most 604800 (7 days)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let list = targets(app, gid, members, true).await?;
    let bulk = list.len() > 1;
    let mut problems = preflight::check_tools(&s, ["member_ban"]);
    if bulk && !s.bot.permissions.contains(Permission::ManageGuild) && s.bot.user_id != s.guild.owner_id {
        problems.push(PreflightProblem::MissingPermission {
            perm: Permission::ManageGuild.name().to_owned(),
            needed_by: "member_ban for more than one member (bulk ban)".to_owned(),
        });
    }
    refuse_if(&problems)?;
    let mut action = action_for("ban", &list, &s, if bulk { BAN_BATCH } else { SINGLE });
    if delete_message_seconds > 0 {
        action = action.with_note(format!("also delete their messages from the last {delete_message_seconds} seconds"));
    }
    if !confirmed {
        return Ok(BulkResult::Preview(action.preview()));
    }
    let summary = |n: usize| format!("ban {n} member(s)");
    let out = action
        .run(
            |batch: Vec<Target>| async move {
                if let [t] = batch.as_slice() {
                    let r = audit_reason(&format!("ban {}", who(t)), reason);
                    return one(t.clone(), app.port.ban(gid, t.id, delete_message_seconds, &r).await);
                }
                let ids: Vec<Id> = batch.iter().map(|t| t.id).collect();
                let r = audit_reason(&summary(ids.len()), reason);
                match app.port.bulk_ban(gid, &ids, delete_message_seconds, &r).await {
                    Ok(res) => BatchResult::Done {
                        succeeded: batch.iter().filter(|t| res.banned.contains(&t.id)).cloned().collect(),
                        failed: batch
                            .iter()
                            .filter(|t| !res.banned.contains(&t.id))
                            .map(|t| Failure { item: t.clone(), reason: "Discord didn't ban this user".to_owned() })
                            .collect(),
                    },
                    // 500000: nobody in the batch could be banned; the run goes on.
                    Err(PortError::Discord { code: Some(500_000), .. }) => BatchResult::Done {
                        succeeded: Vec::new(),
                        failed: batch
                            .into_iter()
                            .map(|t| Failure {
                                item: t,
                                reason: "Discord couldn't ban any user in this batch".to_owned(),
                            })
                            .collect(),
                    },
                    Err(e) => BatchResult::Fatal(e.to_string()),
                }
            },
            hooks,
        )
        .await;
    Ok(BulkResult::Done(out))
}

/// Lifts bans by user id. Banned users aren't members, so names can't be looked up.
pub async fn ban_remove<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    users: &[String],
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkOutcome<Target>, ServiceError> {
    if users.is_empty() {
        return Err(invalid("give at least one user id"));
    }
    let (gid, s) = writable(app, guild).await?;
    let mut ids = Vec::new();
    for u in users {
        let id = as_id(u).ok_or_else(|| {
            invalid(format!("`{u}` isn't a user id (banned users aren't members, so names can't be looked up)"))
        })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    refuse_if(&preflight::check_tools(&s, ["ban_remove"]))?;
    let list: Vec<Target> = ids.iter().map(|id| Target { id: *id, name: format!("user {id}") }).collect();
    let out = BulkAction::new("unban", list, SINGLE)
        .run(
            |batch: Vec<Target>| async move {
                let t = batch.into_iter().next().expect("batch of one");
                let r = audit_reason(&format!("unban {}", t.id), reason);
                one(t.clone(), app.port.unban(gid, t.id, &r).await)
            },
            hooks,
        )
        .await;
    Ok(out)
}

/// One ban. `name` and `reason` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BanRow {
    pub user: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A page of bans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BansPage {
    pub bans: Vec<BanRow>,
    /// Pass as `after` for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after: Option<Id>,
}

/// The ban list, with reasons, in pages: `limit` ≤ 1000 (default 100), `before`/`after` ids.
pub async fn bans_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    limit: Option<u16>,
    before: Option<Id>,
    after: Option<Id>,
) -> Result<BansPage, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["bans_list"]))?;
    let limit = limit.unwrap_or(100).clamp(1, 1000);
    let bans = app.port.bans(gid, limit, before, after).await?;
    let next_after = (bans.len() == usize::from(limit)).then(|| bans.last().map(|b| b.user_id)).flatten();
    Ok(BansPage {
        bans: bans.into_iter().map(|b| BanRow { user: b.user_id, name: b.username, reason: b.reason }).collect(),
        next_after,
    })
}

// ---- timeouts and kicks ------------------------------------------------------------------------

/// Times members out for `seconds` (≤ 28 days); `0` clears it. Administrators and the owner
/// can't be timed out.
pub async fn member_timeout<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    members: &[String],
    seconds: u64,
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkOutcome<Target>, ServiceError> {
    if seconds > MAX_TIMEOUT_SECS {
        return Err(invalid("a timeout can be at most 28 days (2419200 seconds)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let list = targets(app, gid, members, false).await?;
    refuse_if(&preflight::check_tools(&s, ["member_timeout"]))?;
    let mut action = action_for("timeout", &list, &s, SINGLE);
    for (t, m) in &list {
        let admin = m.as_ref().is_some_and(|m| {
            m.role_ids.iter().filter_map(|r| s.role(*r)).any(|r| r.permissions.contains(Permission::Administrator))
        });
        if admin && hierarchy_problem(&s, t, m.as_ref()).is_none() {
            action = action.with_problem(t.clone(), Problem::Other("administrators can't be timed out".to_owned()));
        }
    }
    let until = (seconds > 0).then(|| date::rfc3339_after(app.clock.as_ref(), seconds));
    let out = action
        .run(
            |batch: Vec<Target>| {
                let until = until.clone();
                async move {
                    let t = batch.into_iter().next().expect("batch of one");
                    let summary = match &until {
                        Some(u) => format!("time out {} until {u}", who(&t)),
                        None => format!("clear timeout for {}", who(&t)),
                    };
                    let res = app.port.timeout(gid, t.id, until.as_deref(), &audit_reason(&summary, reason)).await;
                    one(t, res)
                }
            },
            hooks,
        )
        .await;
    Ok(out)
}

/// Kicks members after confirmation.
pub async fn member_kick<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    members: &[String],
    confirmed: bool,
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkResult<Target>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let list = targets(app, gid, members, false).await?;
    refuse_if(&preflight::check_tools(&s, ["member_kick"]))?;
    let action = action_for("kick", &list, &s, SINGLE).with_note("they can rejoin with an invite");
    if !confirmed {
        return Ok(BulkResult::Preview(action.preview()));
    }
    let out = action
        .run(
            |batch: Vec<Target>| async move {
                let t = batch.into_iter().next().expect("batch of one");
                let r = audit_reason(&format!("kick {}", who(&t)), reason);
                let res = app.port.kick(gid, t.id, &r).await;
                one(t, res)
            },
            hooks,
        )
        .await;
    Ok(BulkResult::Done(out))
}

// ---- prune -----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Pruned {
    /// How many were removed; `null` on large servers, where Discord doesn't count.
    pub pruned: Option<u64>,
}

/// Removes members inactive for `days` (1–30) who have no roles, or only roles in `roles`.
/// The preview is Discord's own count.
pub async fn members_prune<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    days: u8,
    roles: &[String],
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Pruned>, ServiceError> {
    if !(1..=30).contains(&days) {
        return Err(invalid("days must be 1–30"));
    }
    let (gid, s) = writable(app, guild).await?;
    let role_ids =
        roles.iter().map(|r| resolved(resolve::resolve_role(&s, r), "role", r)).collect::<Result<Vec<Id>, _>>()?;
    refuse_if(&preflight::check_tools(&s, ["members_prune"]))?;
    let count = app.port.prune_count(gid, days, &role_ids).await?;
    let scope = if role_ids.is_empty() {
        "with no roles".to_owned()
    } else {
        let names: Vec<String> = role_ids.iter().map(|r| role_name(&s, *r)).collect();
        format!("with no roles other than {}", names.join(", "))
    };
    let what = format!("remove {count} member(s) inactive for {days} day(s) {scope}");
    confirm::guarded(confirmed, vec![what.clone()], || async {
        // Counting a large prune can time out, so Discord is asked not to (research E4).
        let pruned = app.port.prune(gid, days, &role_ids, count < 1000, &audit_reason(&what, reason)).await?;
        Ok(Pruned { pruned })
    })
    .await
}

// ---- purge -----------------------------------------------------------------------------------

/// A message a purge would delete. `author` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PurgeTarget {
    pub id: Id,
    pub author: String,
    /// RFC 3339.
    pub at: String,
}

/// A message id, or an RFC 3339 time.
fn bound(input: Option<&str>) -> Result<Option<Result<Id, i64>>, ServiceError> {
    let Some(raw) = input.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
    if let Ok(id) = raw.parse::<Id>() {
        return Ok(Some(Ok(id)));
    }
    date::parse_rfc3339(raw)
        .map(|t| Some(Err(t)))
        .ok_or_else(|| invalid(format!("`{raw}` is neither a message id nor an RFC 3339 time with an offset")))
}

fn ts(m: &Message) -> i64 {
    date::parse_rfc3339(&m.timestamp).unwrap_or(0)
}

/// The newest messages matching the filters, at most `count`, reading at most 10 pages per
/// 100 wanted.
async fn scan<P: DiscordPort>(
    app: &App<P>,
    channel: Id,
    author: Option<Id>,
    count: usize,
    after: Option<Result<Id, i64>>,
    before: Option<Result<Id, i64>>,
) -> Result<Vec<Message>, ServiceError> {
    let mut found: Vec<Message> = Vec::new();
    let mut cursor = match before {
        Some(Ok(id)) => Some(id),
        _ => None,
    };
    for _ in 0..count.div_ceil(100) * 10 {
        let page = app.port.read_messages(channel, 100, cursor).await?;
        let Some(last) = page.last() else { break };
        cursor = Some(last.id);
        for m in page {
            match after {
                Some(Ok(id)) if m.id <= id => return Ok(found),
                Some(Err(t)) if ts(&m) <= t => return Ok(found),
                _ => {}
            }
            if matches!(before, Some(Err(t)) if ts(&m) >= t) {
                continue;
            }
            if author.is_none_or(|a| m.author_id == a) {
                found.push(m);
                if found.len() == count {
                    return Ok(found);
                }
            }
        }
    }
    Ok(found)
}

/// Deletes recent messages in a channel, optionally only one author's, at most `count`
/// (default 100, ≤ 1000), within `after`/`before` (message ids or times). Messages newer than
/// 14 days go 100 per request; older ones are reported, and deleted one by one only with
/// `include_old` (E10).
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn messages_purge<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    author: Option<&str>,
    count: Option<usize>,
    after: Option<&str>,
    before: Option<&str>,
    include_old: bool,
    confirmed: bool,
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkResult<PurgeTarget>, ServiceError> {
    let count = count.unwrap_or(100);
    if !(1..=PURGE_MAX).contains(&count) {
        return Err(invalid("count must be 1–1000"));
    }
    let (after, before) = (bound(after)?, bound(before)?);
    let (gid, s) = writable(app, guild).await?;
    let cid = text_channel(&s, channel)?;
    let author_id = match author {
        Some(a) => Some(match as_id(a) {
            Some(id) => id,
            None => find_member(app, gid, a).await?.id,
        }),
        None => None,
    };
    refuse_if(&preflight::check_tools(&s, ["messages_purge"]))?;
    let found = scan(app, cid, author_id, count, after, before).await?;

    let now = date::now_secs(app.clock.as_ref());
    // A minute's margin, so nothing ages past 14 days between the preview and the run.
    let is_old = |m: &Message| now - ts(m) > BULK_DELETE_MAX_AGE - 60;
    let row = |m: &Message| PurgeTarget { id: m.id, author: m.author_name.clone(), at: m.timestamp.clone() };
    let recent: Vec<PurgeTarget> = found.iter().filter(|m| !is_old(m)).map(row).collect();
    let old: Vec<PurgeTarget> = found.iter().filter(|m| is_old(m)).map(row).collect();

    let mut first = BulkAction::new("delete message", recent, DELETE_BATCH);
    let mut second = BulkAction::new("delete old message", Vec::new(), SINGLE);
    if include_old {
        second = BulkAction::new("delete old message", old.clone(), SINGLE);
        if !old.is_empty() {
            first = first.with_note(format!(
                "{} message(s) are older than 14 days and will be deleted one by one ({} extra requests)",
                old.len(),
                old.len()
            ));
        }
    } else {
        for m in &old {
            first = first.with_problem(m.clone(), Problem::TooOld);
        }
        if !old.is_empty() {
            first = first.with_note(format!(
                "{} message(s) are older than 14 days; pass include_old to delete them one by one",
                old.len()
            ));
        }
    }
    if !confirmed {
        let mut p = first.preview();
        let extra = second.preview();
        p.total += extra.total;
        let room = crate::bulk::PREVIEW_LIMIT.saturating_sub(p.targets.len());
        p.targets.extend(extra.targets.iter().take(room).cloned());
        p.more += extra.total - extra.targets.len().min(room);
        p.estimated_requests += extra.estimated_requests;
        return Ok(BulkResult::Preview(p));
    }
    let summary = |n: usize| format!("purge {n} message(s) in #{}", s.channel(cid).map_or("", |c| c.name.as_str()));
    let mut out = first
        .run(
            |batch: Vec<PurgeTarget>| async move {
                let r = audit_reason(&summary(batch.len()), reason);
                if let [m] = batch.as_slice() {
                    return one(m.clone(), app.port.delete_message(cid, m.id, &r).await);
                }
                let ids: Vec<Id> = batch.iter().map(|m| m.id).collect();
                match app.port.bulk_delete(cid, &ids, &r).await {
                    Ok(()) => BatchResult::Done { succeeded: batch, failed: Vec::new() },
                    Err(e) if is_fatal(&e) => BatchResult::Fatal(e.to_string()),
                    Err(e) => BatchResult::Done {
                        succeeded: Vec::new(),
                        failed: batch.into_iter().map(|item| Failure { item, reason: e.to_string() }).collect(),
                    },
                }
            },
            hooks,
        )
        .await;
    if out.state == BulkState::Completed {
        let rest = second
            .run(
                |batch: Vec<PurgeTarget>| async move {
                    let m = batch.into_iter().next().expect("batch of one");
                    let r = audit_reason(&format!("delete message {} (older than 14 days)", m.id), reason);
                    let res = app.port.delete_message(cid, m.id, &r).await;
                    one(m, res)
                },
                hooks,
            )
            .await;
        out.state = rest.state;
        out.succeeded.extend(rest.succeeded);
        out.failed.extend(rest.failed);
        out.not_attempted.extend(rest.not_attempted);
    } else {
        out.not_attempted.extend(second.runnable());
    }
    Ok(BulkResult::Done(out))
}

// ---- bulk roles --------------------------------------------------------------------------------

/// One member's role change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RoleChange {
    pub member: Target,
    /// `add` or `remove`.
    pub change: String,
}

/// Gives a role to many members and takes it from others. Members who already have (or lack)
/// it cost no request.
pub async fn roles_bulk_assign<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    role: &str,
    add: &[String],
    remove: &[String],
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkOutcome<RoleChange>, ServiceError> {
    if add.is_empty() && remove.is_empty() {
        return Err(invalid("give members to `add` or `remove`"));
    }
    let (gid, s) = writable(app, guild).await?;
    let rid = resolved(resolve::resolve_role(&s, role), "role", role)?;
    if rid == s.guild.id || s.role(rid).is_some_and(|r| r.managed) {
        return Err(invalid(format!("{} can't be given or taken away", role_name(&s, rid))));
    }
    let mut problems = preflight::check_tools(&s, ["roles_bulk_assign"]);
    problems.extend(preflight::check_role_edit(&s, rid));
    refuse_if(&problems)?;
    let mut changes = Vec::new();
    for (inputs, change) in [(add, "add"), (remove, "remove")] {
        if inputs.is_empty() {
            continue;
        }
        for (t, m) in targets(app, gid, inputs, false).await? {
            let has = m.as_ref().is_some_and(|m| m.role_ids.contains(&rid));
            changes.push((RoleChange { member: t, change: change.to_owned() }, has));
        }
    }
    let label = role_name(&s, rid);
    let list: Vec<RoleChange> = changes.iter().map(|(c, _)| c.clone()).collect();
    let out = BulkAction::new("role change", list, SINGLE)
        .run(
            |batch: Vec<RoleChange>| {
                let label = label.clone();
                let changes = &changes;
                async move {
                    let c = batch.into_iter().next().expect("batch of one");
                    let has = changes.iter().any(|(x, h)| *x == c && *h);
                    let adding = c.change == "add";
                    if has == adding {
                        return BatchResult::single(c, Ok(()));
                    }
                    let (verb, prep) = if adding { ("give", "to") } else { ("take", "from") };
                    let r = audit_reason(&format!("{verb} {label} {prep} {}", who(&c.member)), reason);
                    let res = if adding {
                        app.port.add_member_role(gid, c.member.id, rid, &r).await
                    } else {
                        app.port.remove_member_role(gid, c.member.id, rid, &r).await
                    };
                    one(c, res)
                }
            },
            hooks,
        )
        .await;
    Ok(out)
}

// ---- audit log ---------------------------------------------------------------------------------

/// Discord's audit log action types, by name.
pub const AUDIT_ACTIONS: &[(u16, &str)] = &[
    (1, "guild_update"),
    (10, "channel_create"),
    (11, "channel_update"),
    (12, "channel_delete"),
    (13, "channel_overwrite_create"),
    (14, "channel_overwrite_update"),
    (15, "channel_overwrite_delete"),
    (20, "member_kick"),
    (21, "member_prune"),
    (22, "member_ban_add"),
    (23, "member_ban_remove"),
    (24, "member_update"),
    (25, "member_role_update"),
    (26, "member_move"),
    (27, "member_disconnect"),
    (28, "bot_add"),
    (30, "role_create"),
    (31, "role_update"),
    (32, "role_delete"),
    (40, "invite_create"),
    (41, "invite_update"),
    (42, "invite_delete"),
    (50, "webhook_create"),
    (51, "webhook_update"),
    (52, "webhook_delete"),
    (60, "emoji_create"),
    (61, "emoji_update"),
    (62, "emoji_delete"),
    (72, "message_delete"),
    (73, "message_bulk_delete"),
    (74, "message_pin"),
    (75, "message_unpin"),
    (80, "integration_create"),
    (81, "integration_update"),
    (82, "integration_delete"),
    (83, "stage_instance_create"),
    (84, "stage_instance_update"),
    (85, "stage_instance_delete"),
    (90, "sticker_create"),
    (91, "sticker_update"),
    (92, "sticker_delete"),
    (100, "guild_scheduled_event_create"),
    (101, "guild_scheduled_event_update"),
    (102, "guild_scheduled_event_delete"),
    (110, "thread_create"),
    (111, "thread_update"),
    (112, "thread_delete"),
    (121, "application_command_permission_update"),
    (130, "soundboard_sound_create"),
    (131, "soundboard_sound_update"),
    (132, "soundboard_sound_delete"),
    (140, "auto_moderation_rule_create"),
    (141, "auto_moderation_rule_update"),
    (142, "auto_moderation_rule_delete"),
    (143, "auto_moderation_block_message"),
    (144, "auto_moderation_flag_to_channel"),
    (145, "auto_moderation_user_communication_disabled"),
    (150, "creator_monetization_request_created"),
    (151, "creator_monetization_terms_accepted"),
    (163, "onboarding_prompt_create"),
    (164, "onboarding_prompt_update"),
    (165, "onboarding_prompt_delete"),
    (166, "onboarding_create"),
    (167, "onboarding_update"),
    (190, "home_settings_create"),
    (191, "home_settings_update"),
    (192, "voice_channel_status_update"),
    (193, "voice_channel_status_delete"),
];

fn action_name(t: u16) -> String {
    AUDIT_ACTIONS.iter().find(|(n, _)| *n == t).map_or_else(|| format!("action_{t}"), |(_, name)| (*name).to_owned())
}

/// One audit log entry. `reason` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AuditRow {
    pub id: Id,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
}

/// The audit log, newest first: filter by action (name like `member_ban_add`, or its number)
/// and by who did it; page with `before` (an entry id); `limit` ≤ 100 (default 50).
pub async fn audit_log<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    action_type: Option<&str>,
    user: Option<&str>,
    before: Option<Id>,
    limit: Option<u8>,
) -> Result<Vec<AuditRow>, ServiceError> {
    let action = action_type
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.parse::<u16>().ok().or_else(|| AUDIT_ACTIONS.iter().find(|(_, n)| *n == a).map(|(t, _)| *t)).ok_or_else(
                || {
                    let names = AUDIT_ACTIONS.iter().map(|(_, n)| *n);
                    let mut hint: Vec<String> =
                        names.clone().filter(|n| n.contains(a)).take(3).map(str::to_owned).collect();
                    if hint.is_empty() {
                        hint = guildsmith_core::suggest::closest(a, names, 3);
                    }
                    let hint =
                        if hint.is_empty() { String::new() } else { format!("; did you mean {}?", hint.join(", ")) };
                    invalid(format!("unknown action `{a}`{hint}"))
                },
            )
        })
        .transpose()?;
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["audit_log"]))?;
    let user = match user.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(match as_id(u) {
            Some(id) => id,
            None => find_member(app, gid, u).await?.id,
        }),
        None => None,
    };
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let entries = app.port.audit_log(gid, action, user, before, limit).await?;
    Ok(entries
        .into_iter()
        .map(|e| AuditRow {
            id: e.id,
            action: action_name(e.action_type),
            user: e.user_id,
            target: e.target_id,
            reason: e.reason,
            changes: e.changes,
        })
        .collect())
}

// ---- raid lockdown -----------------------------------------------------------------------------

/// Pauses invites and/or DMs for up to 24 hours each, or clears both, after confirmation.
pub async fn raid_lockdown<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    pause_invites_for: Option<u64>,
    pause_dms_for: Option<u64>,
    clear: bool,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Incidents>, ServiceError> {
    if clear && (pause_invites_for.is_some() || pause_dms_for.is_some()) {
        return Err(invalid("use either clear or pause_*_for, not both"));
    }
    if !clear && pause_invites_for.is_none() && pause_dms_for.is_none() {
        return Err(invalid("give pause_invites_for and/or pause_dms_for (seconds, up to 86400), or clear: true"));
    }
    for v in [pause_invites_for, pause_dms_for].into_iter().flatten() {
        if v == 0 || v > MAX_LOCKDOWN_SECS {
            return Err(invalid("a pause lasts 1–86400 seconds (at most 24 hours)"));
        }
    }
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["raid_lockdown"]))?;
    let at = |secs: Option<u64>| secs.map(|x| date::rfc3339_after(app.clock.as_ref(), x));
    let want = Incidents { invites_disabled_until: at(pause_invites_for), dms_disabled_until: at(pause_dms_for) };
    let mut would = Vec::new();
    if clear {
        would.push("turn invites and DMs back on now".to_owned());
    }
    if let Some(t) = &want.invites_disabled_until {
        would.push(format!("pause new invites until {t}"));
    }
    if let Some(t) = &want.dms_disabled_until {
        would.push(format!("pause DMs between members until {t}"));
    }
    let summary = format!("raid lockdown: {}", would.join("; "));
    confirm::guarded(confirmed, would, || async {
        Ok(app.port.set_incidents(gid, &want, &audit_reason(&summary, reason)).await?)
    })
    .await
}
