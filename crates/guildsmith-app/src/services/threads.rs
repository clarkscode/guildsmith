//! The `threads` group (US2, FR-021–023): threads, forum and media posts, thread members and
//! forum tags. Thread names are user-written: tools return them wrapped as untrusted.

use guildsmith_core::Id;
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::spec::{ChannelKind, ForumTag};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::assets::Caller;
use crate::confirm::{self, Outcome};
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{ChannelFields, DiscordPort, NewThread, PortError, ThreadEdit, ThreadInfo};
use crate::preflight;
use crate::resolve;
use crate::services::live::{Deleted, find_member, invalid, refuse_if, resolved, server, writable};
use crate::services::messages::{ComposeInput, build_message};

/// Auto-archive durations Discord accepts, in minutes.
pub const AUTO_ARCHIVE: [u32; 4] = [60, 1440, 4320, 10_080];
/// A forum's `REQUIRE_TAG` flag.
pub const REQUIRE_TAG: u64 = 1 << 4;
pub const MAX_FORUM_TAGS: usize = 20;
pub const MAX_POST_TAGS: usize = 5;

/// A thread as tools return it. `name` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThreadRow {
    pub id: Id,
    pub parent: String,
    pub name: String,
    pub kind: String,
    pub archived: bool,
    pub locked: bool,
    pub auto_archive: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub messages: u32,
    pub members: u32,
}

fn row(s: &Snapshot, t: &ThreadInfo) -> ThreadRow {
    let parent = s.channel(t.parent_id);
    let tag_name = |id: &Id| {
        parent.and_then(|p| p.tags.iter().find(|x| x.id == *id)).map_or_else(|| id.to_string(), |x| x.name.clone())
    };
    ThreadRow {
        id: t.id,
        parent: parent.map_or_else(|| t.parent_id.to_string(), |p| format!("#{}", p.name)),
        name: t.name.clone(),
        kind: t.kind.clone(),
        archived: t.archived,
        locked: t.locked,
        auto_archive: t.auto_archive,
        tags: t.tags.iter().map(tag_name).collect(),
        messages: t.message_count,
        members: t.member_count,
    }
}

fn check_name(name: &str) -> Result<(), ServiceError> {
    if (1..=100).contains(&name.trim().chars().count()) {
        Ok(())
    } else {
        Err(invalid("thread names are 1–100 characters"))
    }
}

fn check_archive(minutes: Option<u32>) -> Result<(), ServiceError> {
    match minutes {
        Some(m) if !AUTO_ARCHIVE.contains(&m) => {
            Err(invalid("auto_archive is 60, 1440, 4320 or 10080 minutes (1 hour, 1 day, 3 days, 1 week)"))
        }
        _ => Ok(()),
    }
}

/// Tag names (or ids) on a forum → ids. Unknown names fail with the forum's tags listed.
fn tag_ids(s: &Snapshot, forum: Id, names: &[String]) -> Result<Vec<Id>, ServiceError> {
    let tags = &s.channel(forum).expect("resolved forum").tags;
    if names.len() > MAX_POST_TAGS {
        return Err(invalid("a post can have at most 5 tags"));
    }
    names
        .iter()
        .map(|n| {
            let n = n.trim();
            tags.iter().find(|t| t.name.eq_ignore_ascii_case(n) || t.id.to_string() == n).map(|t| t.id).ok_or_else(
                || {
                    let valid: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
                    let list = if valid.is_empty() {
                        "this forum has no tags".to_owned()
                    } else {
                        format!("valid tags: {}", valid.join(", "))
                    };
                    invalid(format!("unknown tag `{n}`; {list}"))
                },
            )
        })
        .collect()
}

/// A thread by id (`<#id>`) or exact name among active threads. It must be in this server.
async fn find_thread<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    s: &Snapshot,
    input: &str,
) -> Result<ThreadInfo, ServiceError> {
    let raw = input.trim().trim_start_matches("<#").trim_end_matches('>');
    let found = if let Ok(id) = raw.parse::<Id>() {
        app.port.thread(id).await.map_err(|e| match e {
            PortError::NotFound(_) => {
                ServiceError::NotFound { what: "thread", input: input.to_owned(), suggestions: Vec::new() }
            }
            other => other.into(),
        })?
    } else {
        let name = raw.strip_prefix('#').unwrap_or(raw);
        let active = app.port.active_threads(gid).await?;
        let hits: Vec<&ThreadInfo> = active.iter().filter(|t| t.name == name).collect();
        match hits.as_slice() {
            [one] => (*one).clone(),
            [] => {
                return Err(ServiceError::NotFound {
                    what: "active thread",
                    input: input.to_owned(),
                    suggestions: guildsmith_core::suggest::closest(name, active.iter().map(|t| t.name.as_str()), 3),
                });
            }
            many => {
                return Err(ServiceError::Ambiguous {
                    what: "thread",
                    input: input.to_owned(),
                    candidates: many
                        .iter()
                        .map(|t| resolve::Candidate {
                            name: t.name.clone(),
                            id: t.id,
                            parent: s.channel(t.parent_id).map(|p| p.name.clone()),
                        })
                        .collect(),
                });
            }
        }
    };
    if s.channel(found.parent_id).is_none() {
        return Err(ServiceError::NotFound { what: "thread", input: input.to_owned(), suggestions: Vec::new() });
    }
    Ok(found)
}

// ---- create, update, delete ------------------------------------------------------------------

/// A thread or post to create.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadInput {
    pub name: String,
    pub from_message: Option<Id>,
    pub private: bool,
    pub auto_archive: Option<u32>,
    /// Forum/media posts: tag names.
    pub tags: Vec<String>,
    /// Forum/media posts: the first message.
    pub message: Option<ComposeInput>,
}

/// Starts a thread in a text or announcement channel (optionally on a message), or a post in a
/// forum or media channel (with its first message and tags).
pub async fn thread_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    input: &ThreadInput,
    reason: Option<&str>,
) -> Result<ThreadRow, ServiceError> {
    check_name(&input.name)?;
    check_archive(input.auto_archive)?;
    let (gid, s) = writable(app, guild).await?;
    let cid = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let kind = s.channel(cid).map(|c| c.kind);
    let forum = matches!(kind, Some(LiveChannelKind::Known(k)) if k.is_forum_like());
    let text = matches!(kind, Some(LiveChannelKind::Known(ChannelKind::Text | ChannelKind::Announcement)));
    if !forum && !text {
        return Err(invalid(format!(
            "`{channel}` can't hold threads; use a text, announcement, forum or media channel"
        )));
    }
    let mut t = NewThread {
        name: input.name.trim().to_owned(),
        auto_archive: input.auto_archive,
        private: input.private,
        from_message: input.from_message,
        post: None,
        tags: Vec::new(),
    };
    if forum {
        let msg = input
            .message
            .as_ref()
            .ok_or_else(|| invalid("a forum or media post needs `message` (its first message)"))?;
        if input.private || input.from_message.is_some() {
            return Err(invalid("forum and media posts can't be private or start from a message"));
        }
        t.tags = tag_ids(&s, cid, &input.tags)?;
        if t.tags.is_empty() && app.port.channel_flags(cid).await? & REQUIRE_TAG != 0 {
            let valid: Vec<&str> =
                s.channel(cid).map(|c| c.tags.iter().map(|x| x.name.as_str()).collect()).unwrap_or_default();
            return Err(invalid(format!("this forum requires a tag on every post; valid tags: {}", valid.join(", "))));
        }
        t.post = Some(build_message(app, gid, &s, msg, Caller::Mcp { base: None }).await?);
    } else if !input.tags.is_empty() || input.message.is_some() {
        return Err(invalid("tags and message are for forum and media posts; post in the thread after creating it"));
    }
    let mut problems = preflight::check_tools(&s, ["thread_create"]);
    if input.private && !s.bot.permissions.contains(guildsmith_core::Permission::CreatePrivateThreads) {
        problems.extend(preflight::check_tools(&s, ["thread_update"]));
    }
    refuse_if(&problems)?;
    let what = if forum { "post" } else { "thread" };
    let info = app.port.create_thread(cid, &t, &audit_reason(&format!("create {what} {}", t.name), reason)).await?;
    Ok(row(&s, &info))
}

/// Thread changes. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadUpdate {
    pub name: Option<String>,
    pub archived: Option<bool>,
    pub locked: Option<bool>,
    pub auto_archive: Option<u32>,
    /// Tag names (forum posts).
    pub tags: Option<Vec<String>>,
    pub pinned: Option<bool>,
    pub slowmode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThreadUpdated {
    pub thread: ThreadRow,
    /// What changed; empty when nothing needed to.
    pub changes: Vec<String>,
}

/// Renames, archives, unarchives, locks, re-tags or pins a thread. Only what differs is sent.
pub async fn thread_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    thread: &str,
    input: &ThreadUpdate,
    reason: Option<&str>,
) -> Result<ThreadUpdated, ServiceError> {
    if let Some(n) = &input.name {
        check_name(n)?;
    }
    check_archive(input.auto_archive)?;
    if input.slowmode.is_some_and(|s| s > 21_600) {
        return Err(invalid("slowmode is at most 21600 seconds (6 hours)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let t = find_thread(app, gid, &s, thread).await?;
    let mut e = ThreadEdit::default();
    let mut changes = Vec::new();
    if let Some(n) = input.name.as_ref().map(|n| n.trim().to_owned()).filter(|n| *n != t.name) {
        changes.push(format!("name {:?} → {n:?}", t.name));
        e.name = Some(n);
    }
    let mut flag = |field: &str, have: bool, want: Option<bool>, slot: &mut Option<bool>| {
        if let Some(w) = want.filter(|w| *w != have) {
            changes.push(format!("{field} {have} → {w}"));
            *slot = Some(w);
        }
    };
    flag("archived", t.archived, input.archived, &mut e.archived);
    flag("locked", t.locked, input.locked, &mut e.locked);
    flag("pinned", t.pinned, input.pinned, &mut e.pinned);
    if let Some(a) = input.auto_archive.filter(|a| *a != t.auto_archive) {
        changes.push(format!("auto_archive {} → {a}", t.auto_archive));
        e.auto_archive = Some(a);
    }
    if let Some(names) = &input.tags {
        let ids = tag_ids(&s, t.parent_id, names)?;
        if ids != t.tags {
            changes.push(format!("tags → {}", names.join(", ")));
            e.tags = Some(ids);
        }
    }
    if let Some(sm) = input.slowmode {
        changes.push(format!("slowmode → {sm}"));
        e.slowmode = Some(sm);
    }
    if changes.is_empty() {
        return Ok(ThreadUpdated { thread: row(&s, &t), changes });
    }
    refuse_if(&preflight::check_tools(&s, ["thread_update"]))?;
    let summary = format!("update thread {}: {}", t.id, changes.join(", "));
    let info = app.port.update_thread(t.id, &e, &audit_reason(&summary, reason)).await?;
    Ok(ThreadUpdated { thread: row(&s, &info), changes })
}

/// Deletes a thread or post and its messages, after confirmation.
pub async fn thread_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    thread: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let t = find_thread(app, gid, &s, thread).await?;
    let what = format!("thread {} ({}) and its {} message(s)", t.name, t.id, t.message_count);
    confirm::guarded(confirmed, vec![format!("delete {what}")], || async {
        refuse_if(&preflight::check_tools(&s, ["thread_delete"]))?;
        app.port.delete_channel(gid, t.id, &audit_reason(&format!("delete thread {}", t.id), reason)).await?;
        Ok(Deleted { deleted: format!("thread {}", t.id) })
    })
    .await
}

// ---- lists and members -----------------------------------------------------------------------

/// Which threads to list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreadState {
    #[default]
    Active,
    ArchivedPublic,
    ArchivedPrivate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThreadsPage {
    pub threads: Vec<ThreadRow>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub more: bool,
}

/// Active threads (in the server, or one channel), or a channel's archived threads.
pub async fn threads_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: Option<&str>,
    state: ThreadState,
    limit: Option<u8>,
    before: Option<&str>,
) -> Result<ThreadsPage, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let cid = channel.map(|c| resolved(resolve::resolve_channel(&s, c), "channel", c)).transpose()?;
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let (threads, more) = match state {
        ThreadState::Active => {
            let all = app.port.active_threads(gid).await?;
            (all.into_iter().filter(|t| cid.is_none_or(|c| t.parent_id == c)).collect::<Vec<_>>(), false)
        }
        ThreadState::ArchivedPublic | ThreadState::ArchivedPrivate => {
            let c = cid.ok_or_else(|| invalid("archived threads are listed per channel; give `channel`"))?;
            app.port.archived_threads(c, state == ThreadState::ArchivedPrivate, before, limit).await?
        }
    };
    Ok(ThreadsPage { threads: threads.iter().map(|t| row(&s, t)).collect(), more })
}

/// What `thread_members` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberAction {
    List,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ThreadMembers {
    pub thread: Id,
    pub members: Vec<Id>,
}

/// Lists a thread's members, or adds or removes one.
pub async fn thread_members<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    thread: &str,
    action: MemberAction,
    member: Option<&str>,
) -> Result<ThreadMembers, ServiceError> {
    let (gid, s) = if action == MemberAction::List { server(app, guild).await? } else { writable(app, guild).await? };
    let t = find_thread(app, gid, &s, thread).await?;
    if action != MemberAction::List {
        let who = member.ok_or_else(|| invalid("`member` is needed to add or remove"))?;
        let m = find_member(app, gid, who).await?;
        if action == MemberAction::Add {
            app.port.add_thread_member(t.id, m.id).await?;
        } else {
            refuse_if(&preflight::check_tools(&s, ["thread_members"]))?;
            app.port.remove_thread_member(t.id, m.id).await?;
        }
    }
    Ok(ThreadMembers { thread: t.id, members: app.port.thread_members(t.id).await? })
}

// ---- forum tags ------------------------------------------------------------------------------

/// What `forum_tags` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagAction {
    Create,
    Rename,
    Delete,
    DefaultReaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ForumTags {
    pub channel: Id,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reaction: Option<String>,
    pub done: String,
}

/// Creates, renames or deletes (after confirmation) a forum's tag, or sets its default
/// reaction. Sends the forum's whole tag list, so existing tags keep their ids.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn forum_tags<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    action: TagAction,
    tag: Option<&str>,
    name: Option<&str>,
    emoji: Option<&str>,
    moderated: Option<bool>,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<ForumTags>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let c = s.channel(cid).expect("resolved channel");
    if !matches!(c.kind, LiveChannelKind::Known(k) if k.is_forum_like()) {
        return Err(invalid(format!("`{channel}` isn't a forum or media channel")));
    }
    let mut tags: Vec<ForumTag> = c
        .tags
        .iter()
        .map(|t| ForumTag {
            id: Some(t.id),
            name: t.name.clone(),
            emoji: t.emoji.clone(),
            moderated: Some(t.moderated),
        })
        .collect();
    let find = |tags: &[ForumTag], t: &str| {
        tags.iter()
            .position(|x| x.name.eq_ignore_ascii_case(t.trim()) || x.id.is_some_and(|i| i.to_string() == t.trim()))
            .ok_or_else(|| {
                let valid: Vec<&str> = tags.iter().map(|x| x.name.as_str()).collect();
                invalid(format!("no tag `{t}`; valid tags: {}", valid.join(", ")))
            })
    };
    let check_tag_name = |n: &str| {
        if (1..=20).contains(&n.trim().chars().count()) {
            Ok(())
        } else {
            Err(invalid("tag names are 1–20 characters"))
        }
    };
    let mut fields = ChannelFields::default();
    let done = match action {
        TagAction::Create => {
            let n = name.or(tag).ok_or_else(|| invalid("`name` is needed to create a tag"))?;
            check_tag_name(n)?;
            if tags.len() >= MAX_FORUM_TAGS {
                return Err(invalid("a forum has at most 20 tags"));
            }
            if tags.iter().any(|t| t.name.eq_ignore_ascii_case(n.trim())) {
                return Err(invalid(format!("the tag `{n}` already exists")));
            }
            tags.push(ForumTag { id: None, name: n.trim().to_owned(), emoji: emoji.map(str::to_owned), moderated });
            fields.tags = Some(tags.clone());
            format!("created tag {}", n.trim())
        }
        TagAction::Rename => {
            let i = find(&tags, tag.ok_or_else(|| invalid("`tag` is the tag to rename"))?)?;
            let n = name.ok_or_else(|| invalid("`name` is the new name"))?;
            check_tag_name(n)?;
            let old = std::mem::replace(&mut tags[i].name, n.trim().to_owned());
            if let Some(e) = emoji {
                tags[i].emoji = Some(e.to_owned());
            }
            if moderated.is_some() {
                tags[i].moderated = moderated;
            }
            fields.tags = Some(tags.clone());
            format!("renamed tag {old} to {}", n.trim())
        }
        TagAction::Delete => {
            let i = find(&tags, tag.ok_or_else(|| invalid("`tag` is the tag to delete"))?)?;
            let gone = tags.remove(i);
            fields.tags = Some(tags.clone());
            format!("deleted tag {} (posts lose it)", gone.name)
        }
        TagAction::DefaultReaction => {
            fields.default_reaction = Some(emoji.map(str::to_owned).filter(|e| !e.is_empty()));
            match emoji.filter(|e| !e.is_empty()) {
                Some(e) => format!("default reaction set to {e}"),
                None => "default reaction cleared".to_owned(),
            }
        }
    };
    let names: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
    let reaction = fields.default_reaction.clone().unwrap_or_else(|| c.default_reaction.clone());
    let apply = || async {
        refuse_if(&preflight::check_tools(&s, ["forum_tags"]))?;
        let summary = format!("forum #{}: {done}", c.name);
        app.port.update_channel(gid, cid, &fields, &audit_reason(&summary, reason)).await?;
        app.invalidate(gid);
        Ok(ForumTags { channel: cid, tags: names.clone(), default_reaction: reaction.clone(), done: done.clone() })
    };
    if action == TagAction::Delete {
        confirm::guarded(confirmed, vec![done.clone()], apply).await
    } else {
        apply().await.map(Outcome::Done)
    }
}
