//! The `polls` group (US2, FR-024): create polls, read per-answer counts and voters, and end
//! the bot's own polls early. Questions and answers are user-written: tools return them
//! wrapped as untrusted.

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::{AllowedMentions, DiscordPort, NewPoll, OutgoingMessage, PollInfo, UserInfo};
use crate::preflight;
use crate::services::live::{Sent, invalid, refuse_if, server, writable};
use crate::services::messages::message_channel;

pub const MAX_QUESTION: usize = 300;
pub const MAX_ANSWER: usize = 55;
pub const MAX_ANSWERS: usize = 10;
/// 32 days.
pub const MAX_HOURS: u32 = 768;

/// Sends a poll: a question (≤ 300 characters), 2–10 answers (≤ 55 each), lasting 1–768 hours.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn poll_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    question: &str,
    answers: &[String],
    duration_hours: u32,
    multiselect: bool,
) -> Result<Sent, ServiceError> {
    let q = question.trim();
    if q.is_empty() || q.chars().count() > MAX_QUESTION {
        return Err(invalid("the question is 1–300 characters"));
    }
    if !(2..=MAX_ANSWERS).contains(&answers.len()) {
        return Err(invalid("a poll has 2–10 answers"));
    }
    if let Some(a) = answers.iter().find(|a| a.trim().is_empty() || a.trim().chars().count() > MAX_ANSWER) {
        return Err(invalid(format!("each answer is 1–55 characters; `{a}` isn't")));
    }
    if !(1..=MAX_HOURS).contains(&duration_hours) {
        return Err(invalid("duration is 1–768 hours (32 days)"));
    }
    let (gid, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    refuse_if(&preflight::check_tools(&s, ["poll_create"]))?;
    let msg = OutgoingMessage {
        poll: Some(NewPoll {
            question: q.to_owned(),
            answers: answers.iter().map(|a| (a.trim().to_owned(), None)).collect(),
            duration_hours,
            multiselect,
        }),
        allowed_mentions: AllowedMentions::default(),
        ..OutgoingMessage::default()
    };
    let m = app.port.post(cid, &msg).await?;
    Ok(Sent { id: m.id, url: format!("https://discord.com/channels/{gid}/{cid}/{}", m.id) })
}

/// A poll's counts, and the voters for one answer when asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PollResults {
    pub message: Id,
    pub poll: PollInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voters: Option<Vec<UserInfo>>,
}

/// Per-answer counts; with `voters_for` (an answer id or its exact text) also who voted for it,
/// in pages of `limit` (≤ 100) after a user id.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn poll_results<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    voters_for: Option<&str>,
    limit: Option<u8>,
    after: Option<Id>,
) -> Result<PollResults, ServiceError> {
    let (_, s) = server(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let m = app.port.message(cid, message).await?;
    let poll = m.poll.ok_or_else(|| invalid(format!("message {message} has no poll")))?;
    let voters = match voters_for.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => {
            let answer =
                poll.answers.iter().find(|a| a.id.to_string() == v || a.text.eq_ignore_ascii_case(v)).ok_or_else(
                    || {
                        let list: Vec<String> = poll.answers.iter().map(|a| format!("{} ({})", a.text, a.id)).collect();
                        invalid(format!("no answer `{v}`; answers: {}", list.join(", ")))
                    },
                )?;
            Some(app.port.answer_voters(cid, message, answer.id, after, limit.unwrap_or(50).clamp(1, 100)).await?)
        }
        None => None,
    };
    Ok(PollResults { message, poll, voters })
}

/// Ends the bot's own poll now. Polls other people sent can't be ended by the bot.
pub async fn poll_end<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
) -> Result<PollResults, ServiceError> {
    let (_, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let m = app.port.message(cid, message).await?;
    if m.poll.is_none() {
        return Err(invalid(format!("message {message} has no poll")));
    }
    if m.author_id != s.bot.user_id {
        return Err(invalid("only polls the bot sent can be ended"));
    }
    let ended = app.port.end_poll(cid, message).await?;
    Ok(PollResults { message, poll: ended.poll.expect("a poll message keeps its poll"), voters: None })
}
