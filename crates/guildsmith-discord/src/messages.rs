//! `MessagesPort` for the real Discord API.
//!
//! Messages are sent and read as JSON through [`crate::raw`] rather than twilight's typed
//! builders: embeds, Components V2 layouts and polls pass through exactly as given, and the
//! new paginated `messages/pins` routes (research E4) are used.

use guildsmith_app::ports::{
    AllowedMentions, AttachmentInfo, MessageEdit, MessageInfo, MessagesPort, OutgoingMessage, PollAnswer, PollInfo,
    PortResult, ReactionInfo, SearchOutcome, SearchQuery, UserInfo,
};
use guildsmith_core::Id;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::request::Method;

use crate::port::TwilightDiscord;
use crate::{ext, raw};

// ---- Discord's JSON -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct RawUser {
    pub id: Id,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEmoji {
    #[serde(default)]
    pub id: Option<Id>,
    #[serde(default)]
    pub name: Option<String>,
}

impl RawEmoji {
    pub(crate) fn label(&self) -> String {
        match (&self.id, &self.name) {
            (Some(id), Some(n)) => format!("{n}:{id}"),
            (Some(id), None) => id.to_string(),
            (None, Some(n)) => n.clone(),
            (None, None) => String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawReaction {
    count: u32,
    #[serde(default)]
    me: bool,
    emoji: RawEmoji,
}

#[derive(Debug, Deserialize)]
struct RawAttachment {
    filename: String,
    size: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct RawReference {
    #[serde(default)]
    message_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct RawPollMedia {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    emoji: Option<RawEmoji>,
}

#[derive(Debug, Deserialize)]
struct RawPollAnswer {
    answer_id: u32,
    poll_media: RawPollMedia,
}

#[derive(Debug, Deserialize)]
struct RawAnswerCount {
    id: u32,
    count: u32,
}

#[derive(Debug, Deserialize)]
struct RawPollResults {
    #[serde(default)]
    is_finalized: bool,
    #[serde(default)]
    answer_counts: Vec<RawAnswerCount>,
}

#[derive(Debug, Deserialize)]
struct RawPoll {
    question: RawPollMedia,
    answers: Vec<RawPollAnswer>,
    #[serde(default)]
    expiry: Option<String>,
    #[serde(default)]
    allow_multiselect: bool,
    #[serde(default)]
    results: Option<RawPollResults>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawMessage {
    id: Id,
    channel_id: Id,
    author: RawUser,
    #[serde(default)]
    content: String,
    timestamp: String,
    #[serde(default)]
    edited_timestamp: Option<String>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    flags: u64,
    #[serde(default)]
    embeds: Vec<Value>,
    #[serde(default)]
    components: Vec<Value>,
    #[serde(default)]
    attachments: Vec<RawAttachment>,
    #[serde(default)]
    reactions: Vec<RawReaction>,
    #[serde(default)]
    message_reference: Option<RawReference>,
    #[serde(default)]
    poll: Option<RawPoll>,
}

impl From<RawMessage> for MessageInfo {
    fn from(m: RawMessage) -> Self {
        let poll = m.poll.map(|p| {
            let counts = p.results.as_ref().map(|r| r.answer_counts.as_slice()).unwrap_or_default();
            PollInfo {
                question: p.question.text.unwrap_or_default(),
                answers: p
                    .answers
                    .into_iter()
                    .map(|a| PollAnswer {
                        id: a.answer_id,
                        text: a.poll_media.text.unwrap_or_default(),
                        emoji: a.poll_media.emoji.map(|e| e.label()),
                        votes: counts.iter().find(|c| c.id == a.answer_id).map_or(0, |c| c.count),
                    })
                    .collect(),
                multiselect: p.allow_multiselect,
                expiry: p.expiry,
                finalized: p.results.is_some_and(|r| r.is_finalized),
            }
        });
        MessageInfo {
            id: m.id,
            channel_id: m.channel_id,
            author_id: m.author.id,
            author_name: m.author.global_name.unwrap_or(m.author.username),
            author_bot: m.author.bot,
            content: m.content,
            timestamp: m.timestamp,
            edited: m.edited_timestamp.is_some(),
            pinned: m.pinned,
            components: (m.flags & (1 << 15) != 0 && !m.components.is_empty()).then_some(Value::Array(m.components)),
            flags: m.flags,
            embeds: m.embeds,
            attachments: m
                .attachments
                .into_iter()
                .map(|a| AttachmentInfo { filename: a.filename, size: a.size, url: a.url })
                .collect(),
            reactions: m
                .reactions
                .into_iter()
                .map(|r| ReactionInfo { emoji: r.emoji.label(), count: r.count, me: r.me })
                .collect(),
            reference: m.message_reference.and_then(|r| r.message_id),
            poll,
        }
    }
}

impl From<RawUser> for UserInfo {
    fn from(u: RawUser) -> Self {
        UserInfo { id: u.id, name: u.global_name.unwrap_or(u.username) }
    }
}

// ---- request bodies ----------------------------------------------------------------------------

fn mentions_json(m: &AllowedMentions) -> Value {
    let parse: Vec<&str> = if m.everyone { vec!["everyone"] } else { Vec::new() };
    json!({
        "parse": parse,
        "users": m.users,
        "roles": m.roles,
        "replied_user": m.replied_user,
    })
}

fn emoji_json(e: &str) -> Value {
    match e.rsplit_once(':') {
        Some((name, id)) if id.parse::<u64>().is_ok() => json!({"id": id, "name": name}),
        _ if e.parse::<u64>().is_ok() => json!({"id": e}),
        _ => json!({"name": e}),
    }
}

/// The JSON for a message to create (also a forum post's first message).
pub(crate) fn message_json(msg: &OutgoingMessage) -> Value {
    let mut body = Map::new();
    if let Some(c) = &msg.content {
        body.insert("content".to_owned(), json!(c));
    }
    if !msg.embeds.is_empty() {
        body.insert("embeds".to_owned(), json!(msg.embeds));
    }
    body.insert("allowed_mentions".to_owned(), mentions_json(&msg.allowed_mentions));
    if let Some(r) = msg.reply_to {
        body.insert("message_reference".to_owned(), json!({"message_id": r, "fail_if_not_exists": false}));
    }
    if let Some((channel, message)) = msg.forward {
        body.insert("message_reference".to_owned(), json!({"type": 1, "channel_id": channel, "message_id": message}));
    }
    if let Some(c) = &msg.components {
        body.insert("components".to_owned(), c.clone());
    }
    if msg.flags != 0 {
        body.insert("flags".to_owned(), json!(msg.flags));
    }
    if let Some(p) = &msg.poll {
        let answers: Vec<Value> = p
            .answers
            .iter()
            .map(|(text, emoji)| {
                let mut media = json!({"text": text});
                if let Some(e) = emoji {
                    media["emoji"] = emoji_json(e);
                }
                json!({"poll_media": media})
            })
            .collect();
        body.insert(
            "poll".to_owned(),
            json!({
                "question": {"text": p.question},
                "answers": answers,
                "duration": p.duration_hours,
                "allow_multiselect": p.multiselect,
                "layout_type": 1,
            }),
        );
    }
    Value::Object(body)
}

/// Sends a message body (with files as multipart) and reads back the message.
pub(crate) async fn send_message(
    http: &twilight_http::Client,
    path: &str,
    body: &Value,
    files: &[(String, Vec<u8>)],
    reason: Option<&str>,
) -> PortResult<raw::Response> {
    if files.is_empty() {
        raw::send(http, Method::Post, path, &[], Some(body), reason).await
    } else {
        raw::send_files(http, Method::Post, path, body, files, reason).await
    }
}

/// An emoji as a path segment: `name:id` for custom emoji, percent-encoded unicode otherwise.
pub(crate) fn emoji_path(e: &str) -> String {
    utf8_percent_encode(e.trim(), NON_ALPHANUMERIC).to_string()
}

#[derive(Debug, Deserialize)]
struct RawPin {
    pinned_at: String,
    message: RawMessage,
}

#[derive(Debug, Deserialize)]
struct RawPins {
    items: Vec<RawPin>,
    #[serde(default)]
    has_more: bool,
}

impl MessagesPort for TwilightDiscord {
    async fn message(&self, channel: Id, message: Id) -> PortResult<MessageInfo> {
        let http = self.http().await?;
        let m: RawMessage =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{channel}/messages/{message}"), &[], None, None)
                .await?;
        Ok(m.into())
    }

    async fn recent(&self, channel: Id, limit: u8, before: Option<Id>) -> PortResult<Vec<MessageInfo>> {
        let http = self.http().await?;
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(b) = before {
            q.push(("before".to_owned(), b.to_string()));
        }
        let list: Vec<RawMessage> =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{channel}/messages"), &q, None, None).await?;
        Ok(list.into_iter().map(MessageInfo::from).collect())
    }

    async fn post(&self, channel: Id, msg: &OutgoingMessage) -> PortResult<MessageInfo> {
        let http = self.http().await?;
        let files: Vec<(String, Vec<u8>)> = msg.files.iter().map(|f| (f.name.clone(), f.bytes.clone())).collect();
        let res = send_message(http, &format!("channels/{channel}/messages"), &message_json(msg), &files, None).await?;
        Ok(raw::parse::<RawMessage>(&res, "create message")?.into())
    }

    async fn edit(&self, channel: Id, message: Id, edit: &MessageEdit) -> PortResult<MessageInfo> {
        let http = self.http().await?;
        let mut body = Map::new();
        if let Some(c) = &edit.content {
            body.insert("content".to_owned(), json!(c));
        }
        if let Some(e) = &edit.embeds {
            body.insert("embeds".to_owned(), json!(e));
        }
        if let Some(f) = edit.flags {
            body.insert("flags".to_owned(), json!(f));
        }
        if let Some(c) = &edit.components {
            body.insert("components".to_owned(), c.clone());
        }
        let path = format!("channels/{channel}/messages/{message}");
        let res = raw::send(http, Method::Patch, &path, &[], Some(&Value::Object(body)), None).await?;
        Ok(raw::parse::<RawMessage>(&res, "edit message")?.into())
    }

    async fn pins(
        &self,
        channel: Id,
        before: Option<&str>,
        limit: u8,
    ) -> PortResult<(Vec<(MessageInfo, String)>, bool)> {
        let http = self.http().await?;
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(b) = before {
            q.push(("before".to_owned(), b.to_owned()));
        }
        let pins: RawPins =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{channel}/messages/pins"), &q, None, None).await?;
        Ok((pins.items.into_iter().map(|p| (MessageInfo::from(p.message), p.pinned_at)).collect(), pins.has_more))
    }

    async fn set_pinned(&self, channel: Id, message: Id, pinned: bool, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        let method = if pinned { Method::Put } else { Method::Delete };
        raw::send(http, method, &format!("channels/{channel}/messages/pins/{message}"), &[], None, Some(reason))
            .await?;
        Ok(())
    }

    async fn crosspost(&self, channel: Id, message: Id) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Post, &format!("channels/{channel}/messages/{message}/crosspost"), &[], None, None)
            .await?;
        Ok(())
    }

    async fn react(&self, channel: Id, message: Id, emoji: &str) -> PortResult<()> {
        let http = self.http().await?;
        let path = format!("channels/{channel}/messages/{message}/reactions/{}/@me", emoji_path(emoji));
        raw::send(http, Method::Put, &path, &[], None, None).await?;
        Ok(())
    }

    async fn unreact(&self, channel: Id, message: Id, emoji: &str, user: Option<Id>) -> PortResult<()> {
        let http = self.http().await?;
        let who = user.map_or_else(|| "@me".to_owned(), |u| u.to_string());
        let path = format!("channels/{channel}/messages/{message}/reactions/{}/{who}", emoji_path(emoji));
        raw::send(http, Method::Delete, &path, &[], None, None).await?;
        Ok(())
    }

    async fn clear_reactions(&self, channel: Id, message: Id, emoji: Option<&str>) -> PortResult<()> {
        let http = self.http().await?;
        let path = match emoji {
            Some(e) => format!("channels/{channel}/messages/{message}/reactions/{}", emoji_path(e)),
            None => format!("channels/{channel}/messages/{message}/reactions"),
        };
        raw::send(http, Method::Delete, &path, &[], None, None).await?;
        Ok(())
    }

    async fn reaction_users(
        &self,
        channel: Id,
        message: Id,
        emoji: &str,
        after: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<UserInfo>> {
        let http = self.http().await?;
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(a) = after {
            q.push(("after".to_owned(), a.to_string()));
        }
        let path = format!("channels/{channel}/messages/{message}/reactions/{}", emoji_path(emoji));
        let users: Vec<RawUser> = raw::json::<(), _>(http, Method::Get, &path, &q, None, None).await?;
        Ok(users.into_iter().map(UserInfo::from).collect())
    }

    async fn search(&self, guild: Id, query: &SearchQuery) -> PortResult<SearchOutcome> {
        ext::search::search(self.http().await?, guild, query).await
    }
}
