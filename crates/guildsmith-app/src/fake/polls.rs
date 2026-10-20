//! `PollsPort` for the fake: votes per answer, and ending only the bot's own polls.

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::Id;

use super::messages::MessagesState;
use super::{FakeDiscord, not_found};
use crate::ports::{MessageInfo, NewPoll, PollAnswer, PollInfo, PollsPort, PortError, PortResult, UserInfo};

/// A poll held by the fake.
#[derive(Debug, Clone, Default)]
pub struct FakePoll {
    pub question: String,
    pub answers: Vec<(String, Option<String>)>,
    /// Answer id (1-based) → voters.
    pub votes: BTreeMap<u32, BTreeSet<Id>>,
    pub multiselect: bool,
    pub expiry: Option<String>,
    pub finalized: bool,
}

impl FakePoll {
    pub(crate) fn new(p: &NewPoll) -> Self {
        FakePoll {
            question: p.question.clone(),
            answers: p.answers.clone(),
            votes: BTreeMap::new(),
            multiselect: p.multiselect,
            expiry: Some(format!("+{}h", p.duration_hours)),
            finalized: false,
        }
    }

    pub(crate) fn info(&self) -> PollInfo {
        PollInfo {
            question: self.question.clone(),
            answers: self
                .answers
                .iter()
                .enumerate()
                .map(|(i, (text, emoji))| {
                    let id = u32::try_from(i + 1).unwrap_or(u32::MAX);
                    PollAnswer {
                        id,
                        text: text.clone(),
                        emoji: emoji.clone(),
                        votes: self.votes.get(&id).map_or(0, |v| u32::try_from(v.len()).unwrap_or(u32::MAX)),
                    }
                })
                .collect(),
            multiselect: self.multiselect,
            expiry: self.expiry.clone(),
            finalized: self.finalized,
        }
    }
}

impl FakeDiscord {
    /// Records a vote, as if a user voted in Discord.
    pub fn vote(&self, guild: Id, message: Id, user: Id, answer: u32) {
        self.edit_res::<MessagesState>(guild, |s| {
            if let Some(p) = s.extra.get_mut(&message).and_then(|e| e.poll.as_mut()) {
                p.votes.entry(answer).or_default().insert(user);
            }
        });
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl PollsPort for FakeDiscord {
    async fn answer_voters(
        &self,
        channel: Id,
        message: Id,
        answer: u32,
        after: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<UserInfo>> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("answer_voters", guild, Some(message), |g| {
            let poll = g.info(channel, message)?.poll.ok_or_else(|| not_found("poll on message", message))?;
            if !poll.answers.iter().any(|a| a.id == answer) {
                return Err(not_found("poll answer", Id(u64::from(answer))));
            }
            let voters = g
                .res
                .peek::<MessagesState>()
                .and_then(|s| s.extra.get(&message))
                .and_then(|e| e.poll.as_ref())
                .and_then(|p| p.votes.get(&answer).cloned())
                .unwrap_or_default();
            Ok(voters
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

    async fn end_poll(&self, channel: Id, message: Id) -> PortResult<MessageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("end_poll", guild, Some(message), None, |g, _| {
            let info = g.info(channel, message)?;
            if info.poll.is_none() {
                return Err(not_found("poll on message", message));
            }
            if info.author_id != g.snapshot.bot.user_id {
                return Err(PortError::Forbidden("only the poll's author can end it".to_owned()));
            }
            if let Some(p) = g.extra_mut(message).poll.as_mut() {
                p.finalized = true;
                p.expiry = Some("2026-10-19T12:00:00Z".to_owned());
            }
            g.info(channel, message)
        })
    }
}
