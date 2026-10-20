//! `MessagesPort` for the fake: pins (at most 250 per channel), reactions, edits (only the bot's
//! own content; flags with Manage Messages), crossposts, Components V2 rules, and a simple
//! search with a switch for "not indexed yet".

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::snapshot::LiveChannelKind;
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission};
use serde_json::Value;

use super::polls::FakePoll;
use super::{FakeDiscord, FakeGuild, next_id, not_found};
use crate::ports::{
    AttachmentInfo, MessageEdit, MessageInfo, MessagesPort, OutgoingMessage, PortError, PortResult, ReactionInfo,
    SearchOutcome, SearchQuery, UserInfo,
};

/// `IS_COMPONENTS_V2`.
pub const COMPONENTS_V2: u64 = 1 << 15;
const MAX_PINS: usize = 250;

/// What the fake knows about a message beyond v1's `Message`.
#[derive(Debug, Clone, Default)]
pub struct Extra {
    pub author_bot: bool,
    pub edited: bool,
    /// RFC 3339 pin time, when pinned.
    pub pinned_at: Option<String>,
    pub flags: u64,
    pub embeds: Vec<Value>,
    pub components: Option<Value>,
    pub attachments: Vec<AttachmentInfo>,
    /// Emoji → users who reacted.
    pub reactions: BTreeMap<String, BTreeSet<Id>>,
    pub reference: Option<Id>,
    pub poll: Option<FakePoll>,
    pub crossposted: bool,
}

/// The fake's message state for one server.
#[derive(Debug, Clone, Default)]
pub struct MessagesState {
    pub extra: BTreeMap<Id, Extra>,
    /// Search answers "not indexed yet" with this `retry_after`.
    pub not_indexed: Option<u64>,
}

impl FakeGuild {
    pub(crate) fn info(&self, channel: Id, id: Id) -> PortResult<MessageInfo> {
        let m = self
            .messages
            .get(&channel)
            .and_then(|l| l.iter().find(|m| m.id == id))
            .ok_or_else(|| not_found("message", id))?;
        let extra = self.res.peek::<MessagesState>().and_then(|s| s.extra.get(&id)).cloned().unwrap_or_default();
        let bot = self.snapshot.bot.user_id;
        Ok(MessageInfo {
            id: m.id,
            channel_id: channel,
            author_id: m.author_id,
            author_name: m.author_name.clone(),
            author_bot: extra.author_bot || m.author_id == bot,
            content: m.content.clone(),
            timestamp: m.timestamp.clone(),
            edited: extra.edited,
            pinned: extra.pinned_at.is_some(),
            flags: extra.flags,
            embeds: extra.embeds.clone(),
            components: extra.components.clone(),
            attachments: extra.attachments.clone(),
            reactions: extra
                .reactions
                .iter()
                .filter(|(_, users)| !users.is_empty())
                .map(|(e, users)| ReactionInfo {
                    emoji: e.clone(),
                    count: u32::try_from(users.len()).unwrap_or(u32::MAX),
                    me: users.contains(&bot),
                })
                .collect(),
            reference: extra.reference,
            poll: extra.poll.as_ref().map(FakePoll::info),
        })
    }

    pub(crate) fn extra_mut(&mut self, id: Id) -> &mut Extra {
        self.res.get::<MessagesState>().extra.entry(id).or_default()
    }

    /// Adds a message from the bot.
    pub(crate) fn add_bot_message(&mut self, channel: Id, msg: &OutgoingMessage, next: &mut u64) -> PortResult<Id> {
        let v2 = msg.flags & COMPONENTS_V2 != 0 || msg.components.is_some();
        if v2 && (msg.content.is_some() || !msg.embeds.is_empty() || msg.poll.is_some()) {
            return Err(PortError::Discord {
                status: 400,
                code: Some(50035),
                message: "Components V2 messages can't have content, embeds or a poll".to_owned(),
            });
        }
        let empty = msg.content.as_deref().is_none_or(str::is_empty)
            && msg.embeds.is_empty()
            && msg.files.is_empty()
            && msg.poll.is_none()
            && msg.components.is_none()
            && msg.forward.is_none();
        if empty {
            return Err(PortError::Discord {
                status: 400,
                code: Some(50006),
                message: "Cannot send an empty message".to_owned(),
            });
        }
        let id = next_id(next);
        let (bot, name) = (self.snapshot.bot.user_id, "guildsmith".to_owned());
        self.messages.entry(channel).or_default().push(crate::ports::Message {
            id,
            channel_id: channel,
            author_id: bot,
            author_name: name,
            content: msg.content.clone().unwrap_or_default(),
            timestamp: "2026-10-19T12:00:00Z".to_owned(),
        });
        let extra = self.extra_mut(id);
        extra.author_bot = true;
        extra.flags = msg.flags | if v2 { COMPONENTS_V2 } else { 0 };
        extra.embeds.clone_from(&msg.embeds);
        extra.components.clone_from(&msg.components);
        extra.attachments = msg
            .files
            .iter()
            .map(|f| AttachmentInfo {
                filename: f.name.clone(),
                size: f.bytes.len() as u64,
                url: format!("https://cdn.discordapp.com/attachments/{channel}/{id}/{}", f.name),
            })
            .collect();
        extra.reference = msg.reply_to.or(msg.forward.map(|(_, m)| m));
        extra.poll = msg.poll.as_ref().map(FakePoll::new);
        Ok(id)
    }

    fn is_text(&self, channel: Id) -> bool {
        self.snapshot
            .channel(channel)
            .is_some_and(|c| matches!(c.kind, LiveChannelKind::Known(k) if !k.is_forum_like()))
            || self.res.peek::<super::threads::ThreadsState>().is_some_and(|t| t.threads.contains_key(&channel))
    }
}

fn has(info: &MessageInfo, what: &str) -> bool {
    match what {
        "file" | "image" | "video" | "sound" => !info.attachments.is_empty(),
        "embed" => !info.embeds.is_empty(),
        "poll" => info.poll.is_some(),
        "link" => info.content.contains("http://") || info.content.contains("https://"),
        _ => false,
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl MessagesPort for FakeDiscord {
    async fn message(&self, channel: Id, message: Id) -> PortResult<MessageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("message", guild, Some(message), |g| g.info(channel, message))
    }

    async fn recent(&self, channel: Id, limit: u8, before: Option<Id>) -> PortResult<Vec<MessageInfo>> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("recent", guild, None, |g| {
            let all = g.messages.get(&channel).cloned().unwrap_or_default();
            all.iter()
                .rev()
                .filter(|m| before.is_none_or(|b| m.id < b))
                .take(usize::from(limit))
                .map(|m| g.info(channel, m.id))
                .collect()
        })
    }

    async fn post(&self, channel: Id, msg: &OutgoingMessage) -> PortResult<MessageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("post", guild, Some(channel), None, |g, next| {
            if !g.is_text(channel) {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(50008),
                    message: "not a text channel".to_owned(),
                });
            }
            g.bot_can(Permission::SendMessages)?;
            if msg.poll.is_some() {
                g.bot_can(Permission::SendPolls)?;
            }
            let id = g.add_bot_message(channel, msg, next)?;
            g.info(channel, id)
        })
    }

    async fn edit(&self, channel: Id, message: Id, edit: &MessageEdit) -> PortResult<MessageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("edit", guild, Some(message), None, |g, _| {
            let info = g.info(channel, message)?;
            let own = info.author_id == g.snapshot.bot.user_id;
            if !own && (edit.content.is_some() || edit.embeds.is_some() || edit.components.is_some()) {
                return Err(PortError::Forbidden("Cannot edit a message authored by another user".to_owned()));
            }
            if !own && edit.flags.is_some() {
                g.bot_can(Permission::ManageMessages)?;
            }
            if let Some(c) = &edit.content
                && let Some(m) = g.messages.get_mut(&channel).and_then(|l| l.iter_mut().find(|m| m.id == message))
            {
                m.content.clone_from(c);
            }
            let extra = g.extra_mut(message);
            if let Some(e) = &edit.embeds {
                extra.embeds.clone_from(e);
            }
            if let Some(f) = edit.flags {
                extra.flags = f;
            }
            if let Some(c) = &edit.components {
                extra.components = Some(c.clone());
            }
            extra.edited = true;
            g.info(channel, message)
        })
    }

    async fn pins(
        &self,
        channel: Id,
        before: Option<&str>,
        limit: u8,
    ) -> PortResult<(Vec<(MessageInfo, String)>, bool)> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("pins", guild, None, |g| {
            let mut pinned: Vec<(MessageInfo, String)> = g
                .messages
                .get(&channel)
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let at = g.res.peek::<MessagesState>()?.extra.get(&m.id)?.pinned_at.clone()?;
                    Some((g.info(channel, m.id).ok()?, at))
                })
                .filter(|(_, at)| before.is_none_or(|b| at.as_str() < b))
                .collect();
            pinned.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.id.cmp(&a.0.id)));
            let more = pinned.len() > usize::from(limit);
            pinned.truncate(usize::from(limit));
            Ok((pinned, more))
        })
    }

    async fn set_pinned(&self, channel: Id, message: Id, pinned: bool, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("set_pinned", guild, Some(message), Some(reason), |g, _| {
            g.bot_can(Permission::PinMessages)?;
            g.info(channel, message)?;
            let count = g
                .messages
                .get(&channel)
                .into_iter()
                .flatten()
                .filter(|m| {
                    g.res
                        .peek::<MessagesState>()
                        .and_then(|s| s.extra.get(&m.id))
                        .is_some_and(|e| e.pinned_at.is_some())
                })
                .count();
            let extra = g.extra_mut(message);
            if pinned && extra.pinned_at.is_none() {
                if count >= MAX_PINS {
                    return Err(PortError::Discord {
                        status: 400,
                        code: Some(30003),
                        message: "Maximum number of pins reached (250)".to_owned(),
                    });
                }
                extra.pinned_at = Some(format!("2026-10-19T12:{:02}:00Z", count % 60));
            } else if !pinned {
                extra.pinned_at = None;
            }
            Ok(())
        })
    }

    async fn crosspost(&self, channel: Id, message: Id) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("crosspost", guild, Some(message), None, |g, _| {
            let news = g
                .snapshot
                .channel(channel)
                .is_some_and(|c| c.kind == LiveChannelKind::Known(ChannelKind::Announcement));
            if !news {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(40033),
                    message: "This message has already been crossposted or isn't in an announcement channel".to_owned(),
                });
            }
            let info = g.info(channel, message)?;
            if info.author_id != g.snapshot.bot.user_id {
                g.bot_can(Permission::ManageMessages)?;
            }
            g.extra_mut(message).crossposted = true;
            Ok(())
        })
    }

    async fn react(&self, channel: Id, message: Id, emoji: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("react", guild, Some(message), None, |g, _| {
            g.bot_can(Permission::AddReactions)?;
            g.info(channel, message)?;
            let bot = g.snapshot.bot.user_id;
            g.extra_mut(message).reactions.entry(emoji.to_owned()).or_default().insert(bot);
            Ok(())
        })
    }

    async fn unreact(&self, channel: Id, message: Id, emoji: &str, user: Option<Id>) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("unreact", guild, Some(message), None, |g, _| {
            if user.is_some() {
                g.bot_can(Permission::ManageMessages)?;
            }
            g.info(channel, message)?;
            let who = user.unwrap_or(g.snapshot.bot.user_id);
            if let Some(users) = g.extra_mut(message).reactions.get_mut(emoji) {
                users.remove(&who);
            }
            Ok(())
        })
    }

    async fn clear_reactions(&self, channel: Id, message: Id, emoji: Option<&str>) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("clear_reactions", guild, Some(message), None, |g, _| {
            g.bot_can(Permission::ManageMessages)?;
            g.info(channel, message)?;
            let r = &mut g.extra_mut(message).reactions;
            match emoji {
                Some(e) => {
                    r.remove(e);
                }
                None => r.clear(),
            }
            Ok(())
        })
    }

    async fn reaction_users(
        &self,
        channel: Id,
        message: Id,
        emoji: &str,
        after: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<UserInfo>> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("reaction_users", guild, Some(message), |g| {
            g.info(channel, message)?;
            let users = g
                .res
                .peek::<MessagesState>()
                .and_then(|s| s.extra.get(&message))
                .and_then(|e| e.reactions.get(emoji))
                .cloned()
                .unwrap_or_default();
            Ok(users
                .into_iter()
                .filter(|u| after.is_none_or(|a| *u > a))
                .take(usize::from(limit))
                .map(|id| UserInfo {
                    id,
                    name: g
                        .members
                        .iter()
                        .find(|m| m.id == id)
                        .map_or_else(|| format!("user{id}"), |m| m.username.clone()),
                })
                .collect())
        })
    }

    async fn search(&self, guild: Id, q: &SearchQuery) -> PortResult<SearchOutcome> {
        let content_on = self.lock().app_flags & crate::preflight::MESSAGE_CONTENT_FLAGS != 0;
        self.read_guild("search", guild, None, |g| {
            if let Some(retry_after) = g.res.peek::<MessagesState>().and_then(|s| s.not_indexed) {
                return Ok(SearchOutcome::NotIndexed { retry_after });
            }
            let mut hits = Vec::new();
            for (channel, list) in &g.messages {
                if !q.channels.is_empty() && !q.channels.contains(channel) {
                    continue;
                }
                for m in list {
                    let mut info = g.info(*channel, m.id)?;
                    let text_ok =
                        q.content.as_ref().is_none_or(|c| info.content.to_lowercase().contains(&c.to_lowercase()));
                    let ok = text_ok
                        && (q.authors.is_empty() || q.authors.contains(&info.author_id))
                        && q.has.iter().all(|h| has(&info, h))
                        && q.min_id.is_none_or(|i| info.id > i)
                        && q.max_id.is_none_or(|i| info.id < i);
                    if ok {
                        if !content_on {
                            info.content.clear();
                        }
                        hits.push(info);
                    }
                }
            }
            hits.sort_by_key(|m| std::cmp::Reverse(m.id));
            let total = hits.len() as u64;
            let messages = hits.into_iter().skip(q.offset as usize).take(usize::from(q.limit)).collect();
            Ok(SearchOutcome::Found { total, messages })
        })
    }
}

impl FakeDiscord {
    /// Makes search answer "not indexed yet".
    pub fn set_not_indexed(&self, guild: Id, retry_after: Option<u64>) {
        self.edit_res::<MessagesState>(guild, |s| s.not_indexed = retry_after);
    }

    /// Adds a user's reaction, as if they reacted in Discord.
    pub fn react_as(&self, guild: Id, message: Id, user: Id, emoji: &str) {
        self.edit_res::<MessagesState>(guild, |s| {
            s.extra.entry(message).or_default().reactions.entry(emoji.to_owned()).or_default().insert(user);
        });
    }
}
