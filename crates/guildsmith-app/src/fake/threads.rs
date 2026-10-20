//! `ThreadsPort` for the fake: threads and forum/media posts with Discord's rules for names,
//! auto-archive durations and forum tags (at most 5 per post, `REQUIRE_TAG`).

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::snapshot::LiveChannelKind;
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, FakeGuild, next_id, not_found};
use crate::ports::{NewThread, PortError, PortResult, ThreadEdit, ThreadInfo, ThreadsPort};

/// A forum's flag that makes a tag required on every post.
pub const REQUIRE_TAG: u64 = 1 << 4;
const AUTO_ARCHIVE: [u32; 4] = [60, 1440, 4320, 10_080];

/// The fake's thread state for one server.
#[derive(Debug, Clone, Default)]
pub struct ThreadsState {
    pub threads: BTreeMap<Id, ThreadInfo>,
    pub members: BTreeMap<Id, BTreeSet<Id>>,
    /// Channel flags (e.g. `REQUIRE_TAG` on a forum).
    pub flags: BTreeMap<Id, u64>,
}

fn bad(message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(50035), message: message.to_owned() }
}

impl FakeGuild {
    fn thread_info(&self, thread: Id) -> PortResult<ThreadInfo> {
        self.res
            .peek::<ThreadsState>()
            .and_then(|t| t.threads.get(&thread))
            .cloned()
            .ok_or_else(|| not_found("thread", thread))
    }

    fn check_tags(&self, parent: Id, tags: &[Id]) -> PortResult<()> {
        if tags.len() > 5 {
            return Err(bad("a post can have at most 5 tags"));
        }
        let known: Vec<Id> =
            self.snapshot.channel(parent).map(|c| c.tags.iter().map(|t| t.id).collect()).unwrap_or_default();
        if let Some(t) = tags.iter().find(|t| !known.contains(t)) {
            return Err(bad(&format!("unknown tag {t}")));
        }
        Ok(())
    }
}

impl FakeDiscord {
    /// Sets a channel's flags (e.g. `REQUIRE_TAG` on a forum).
    pub fn set_channel_flags(&self, guild: Id, channel: Id, flags: u64) {
        self.edit_res::<ThreadsState>(guild, |t| {
            t.flags.insert(channel, flags);
        });
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl ThreadsPort for FakeDiscord {
    async fn thread(&self, thread: Id) -> PortResult<ThreadInfo> {
        let guild = self.guild_of_channel(thread)?;
        self.read_guild("thread", guild, Some(thread), |g| g.thread_info(thread))
    }

    async fn channel_flags(&self, channel: Id) -> PortResult<u64> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("channel_flags", guild, Some(channel), |g| {
            Ok(g.res.peek::<ThreadsState>().and_then(|t| t.flags.get(&channel).copied()).unwrap_or(0))
        })
    }

    async fn create_thread(&self, channel: Id, t: &NewThread, reason: &str) -> PortResult<ThreadInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("create_thread", guild, Some(channel), Some(reason), |g, next| {
            let parent = g.snapshot.channel(channel).ok_or_else(|| not_found("channel", channel))?;
            let forum = matches!(parent.kind, LiveChannelKind::Known(k) if k.is_forum_like());
            let news = parent.kind == LiveChannelKind::Known(ChannelKind::Announcement);
            g.bot_can(if t.private { Permission::CreatePrivateThreads } else { Permission::CreatePublicThreads })?;
            if !(1..=100).contains(&t.name.chars().count()) {
                return Err(bad("thread names are 1–100 characters"));
            }
            if t.auto_archive.is_some_and(|a| !AUTO_ARCHIVE.contains(&a)) {
                return Err(bad("auto_archive_duration must be 60, 1440, 4320 or 10080"));
            }
            if forum {
                if t.post.is_none() {
                    return Err(bad("a forum post needs a first message"));
                }
                g.check_tags(channel, &t.tags)?;
                let flags = g.res.peek::<ThreadsState>().and_then(|s| s.flags.get(&channel).copied()).unwrap_or(0);
                if flags & REQUIRE_TAG != 0 && t.tags.is_empty() {
                    return Err(bad("A tag is required to create a forum post in this channel"));
                }
            } else if !t.tags.is_empty() || t.post.is_some() {
                return Err(bad("tags and a first message are only for forum and media posts"));
            }
            let id = t.from_message.unwrap_or_else(|| next_id(next));
            let info = ThreadInfo {
                id,
                parent_id: channel,
                name: t.name.clone(),
                kind: if t.private {
                    "private"
                } else if news {
                    "announcement"
                } else {
                    "public"
                }
                .to_owned(),
                archived: false,
                locked: false,
                auto_archive: t.auto_archive.unwrap_or(4320),
                tags: t.tags.clone(),
                pinned: false,
                owner_id: Some(g.snapshot.bot.user_id),
                message_count: u32::from(t.post.is_some()),
                member_count: 1,
            };
            let bot = g.snapshot.bot.user_id;
            let state = g.res.get::<ThreadsState>();
            state.threads.insert(id, info.clone());
            state.members.entry(id).or_default().insert(bot);
            if let Some(post) = &t.post {
                g.add_bot_message(id, post, next)?;
            }
            Ok(info)
        })
    }

    async fn update_thread(&self, thread: Id, e: &ThreadEdit, reason: &str) -> PortResult<ThreadInfo> {
        let guild = self.guild_of_channel(thread)?;
        self.write("update_thread", guild, Some(thread), Some(reason), |g, _| {
            g.bot_can(Permission::ManageThreads)?;
            let current = g.thread_info(thread)?;
            if e.name.as_ref().is_some_and(|n| !(1..=100).contains(&n.chars().count())) {
                return Err(bad("thread names are 1–100 characters"));
            }
            if e.auto_archive.is_some_and(|a| !AUTO_ARCHIVE.contains(&a)) {
                return Err(bad("auto_archive_duration must be 60, 1440, 4320 or 10080"));
            }
            if let Some(tags) = &e.tags {
                g.check_tags(current.parent_id, tags)?;
            }
            let t = g.res.get::<ThreadsState>().threads.get_mut(&thread).expect("checked above");
            if let Some(v) = &e.name {
                t.name.clone_from(v);
            }
            if let Some(v) = e.archived {
                t.archived = v;
            }
            if let Some(v) = e.locked {
                t.locked = v;
            }
            if let Some(v) = e.auto_archive {
                t.auto_archive = v;
            }
            if let Some(v) = &e.tags {
                t.tags.clone_from(v);
            }
            if let Some(v) = e.pinned {
                t.pinned = v;
            }
            Ok(t.clone())
        })
    }

    async fn active_threads(&self, guild: Id) -> PortResult<Vec<ThreadInfo>> {
        self.read_guild("active_threads", guild, None, |g| {
            Ok(g.res
                .peek::<ThreadsState>()
                .map(|t| t.threads.values().filter(|t| !t.archived).cloned().collect())
                .unwrap_or_default())
        })
    }

    async fn archived_threads(
        &self,
        channel: Id,
        private: bool,
        _before: Option<&str>,
        limit: u8,
    ) -> PortResult<(Vec<ThreadInfo>, bool)> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("archived_threads", guild, None, |g| {
            let mut all: Vec<ThreadInfo> = g
                .res
                .peek::<ThreadsState>()
                .map(|t| {
                    t.threads
                        .values()
                        .filter(|t| t.parent_id == channel && t.archived && (t.kind == "private") == private)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            all.sort_by_key(|m| std::cmp::Reverse(m.id));
            let more = all.len() > usize::from(limit);
            all.truncate(usize::from(limit));
            Ok((all, more))
        })
    }

    async fn thread_members(&self, thread: Id) -> PortResult<Vec<Id>> {
        let guild = self.guild_of_channel(thread)?;
        self.read_guild("thread_members", guild, Some(thread), |g| {
            g.thread_info(thread)?;
            Ok(g.res
                .peek::<ThreadsState>()
                .and_then(|t| t.members.get(&thread))
                .map(|m| m.iter().copied().collect())
                .unwrap_or_default())
        })
    }

    async fn add_thread_member(&self, thread: Id, user: Id) -> PortResult<()> {
        let guild = self.guild_of_channel(thread)?;
        self.write("add_thread_member", guild, Some(user), None, |g, _| {
            let t = g.thread_info(thread)?;
            if t.archived {
                return Err(bad("the thread is archived"));
            }
            if !g.members.iter().any(|m| m.id == user) {
                return Err(not_found("member", user));
            }
            let s = g.res.get::<ThreadsState>();
            s.members.entry(thread).or_default().insert(user);
            if let Some(t) = s.threads.get_mut(&thread) {
                t.member_count = u32::try_from(s.members[&thread].len()).unwrap_or(u32::MAX);
            }
            Ok(())
        })
    }

    async fn remove_thread_member(&self, thread: Id, user: Id) -> PortResult<()> {
        let guild = self.guild_of_channel(thread)?;
        self.write("remove_thread_member", guild, Some(user), None, |g, _| {
            g.bot_can(Permission::ManageThreads)?;
            g.thread_info(thread)?;
            let s = g.res.get::<ThreadsState>();
            if let Some(m) = s.members.get_mut(&thread) {
                m.remove(&user);
            }
            Ok(())
        })
    }
}
