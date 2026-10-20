//! The `messages` group (US2, FR-017–020): compose, edit, delete, pin, crosspost, react, and
//! search messages.
//!
//! Message text, author names and embeds are user-written: tools return them wrapped as
//! untrusted. Mentions ping nobody unless the request lists who (FR-019). Messages can also go
//! to a thread (its id), and to the parent channel of a forum post's thread.

use guildsmith_core::asset::{self, Asset, AssetKind};
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::App;
use crate::assets::{self, Caller};
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{
    AllowedMentions, DiscordPort, FileData, MessageEdit, MessageInfo, OutgoingMessage, PortError, SearchOutcome,
    SearchQuery, UserInfo,
};
use crate::preflight::{self, PreflightProblem};
use crate::resolve;
use crate::services::live::{Deleted, Sent, find_member, invalid, refuse_if, resolved, server, writable};

/// `SUPPRESS_EMBEDS`.
pub const SUPPRESS_EMBEDS: u64 = 1 << 2;
/// `IS_COMPONENTS_V2`.
pub const COMPONENTS_V2: u64 = 1 << 15;
pub const MAX_CONTENT: usize = 2000;
pub const MAX_EMBEDS: usize = 10;
pub const MAX_COMPONENTS: usize = 40;
pub const MAX_PINS: usize = 250;
/// The scan fallback reads at most this many messages per channel (FR-020).
pub const SCAN_LIMIT: usize = 500;
/// Discord's epoch for snowflakes, in milliseconds.
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

// ---- shared ----------------------------------------------------------------------------------

/// A message as tools return it. `author`, `content` and `embeds` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MessageView {
    pub id: Id,
    pub channel: Id,
    pub author: String,
    pub content: String,
    pub ts: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl From<&MessageInfo> for MessageView {
    fn from(m: &MessageInfo) -> Self {
        MessageView {
            id: m.id,
            channel: m.channel_id,
            author: m.author_name.clone(),
            content: m.content.clone(),
            ts: m.timestamp.clone(),
            embeds: m.embeds.clone(),
            attachments: m.attachments.iter().map(|a| a.filename.clone()).collect(),
            reactions: m.reactions.iter().map(|r| format!("{} {}", r.emoji, r.count)).collect(),
            pinned: m.pinned,
        }
    }
}

/// A channel that holds messages: a text or announcement channel by name or id, or a thread by
/// id (`<#id>`). Threads must belong to this server.
pub(crate) async fn message_channel<P: DiscordPort>(
    app: &App<P>,
    s: &Snapshot,
    input: &str,
) -> Result<Id, ServiceError> {
    match resolve::resolve_channel(s, input) {
        resolve::Resolution::Resolved(id) => match s.channel(id).map(|c| c.kind) {
            Some(LiveChannelKind::Known(k)) if !k.is_forum_like() && k.is_text_like() => Ok(id),
            Some(LiveChannelKind::Known(ChannelKind::Voice | ChannelKind::Stage)) => Ok(id),
            _ => Err(invalid(format!(
                "`{input}` is not a channel that holds messages; for a forum, use its post's thread"
            ))),
        },
        resolve::Resolution::NotFound { suggestions } => {
            let raw = input.trim().trim_start_matches("<#").trim_end_matches('>');
            if let Ok(id) = raw.parse::<Id>() {
                let t = app.port.thread(id).await.map_err(|e| match e {
                    PortError::NotFound(_) => {
                        ServiceError::NotFound { what: "channel", input: input.to_owned(), suggestions: Vec::new() }
                    }
                    other => other.into(),
                })?;
                if s.channel(t.parent_id).is_some() {
                    return Ok(id);
                }
            }
            Err(ServiceError::NotFound { what: "channel", input: input.to_owned(), suggestions })
        }
        amb @ resolve::Resolution::Ambiguous(_) => resolved(amb, "channel", input),
    }
}

/// Who a message may ping. Everyone off unless `everyone` is true.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionsInput {
    pub users: Vec<String>,
    pub roles: Vec<String>,
    pub everyone: bool,
}

async fn mentions<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    s: &Snapshot,
    input: Option<&MentionsInput>,
    reply: bool,
) -> Result<AllowedMentions, ServiceError> {
    let Some(m) = input else { return Ok(AllowedMentions::default()) };
    let mut users = Vec::new();
    for u in &m.users {
        users.push(match u.trim().trim_start_matches("<@!").trim_start_matches("<@").trim_end_matches('>').parse() {
            Ok(id) => id,
            Err(_) => find_member(app, gid, u).await?.id,
        });
    }
    let roles = m.roles.iter().map(|r| resolved(resolve::resolve_role(s, r), "role", r)).collect::<Result<_, _>>()?;
    Ok(AllowedMentions { users, roles, everyone: m.everyone, replied_user: reply && !m.users.is_empty() })
}

fn count_components(v: &Value) -> usize {
    match v {
        Value::Array(items) => items.iter().map(count_components).sum(),
        Value::Object(map) => {
            let own = usize::from(map.contains_key("type"));
            own + map.values().map(count_components).sum::<usize>()
        }
        _ => 0,
    }
}

/// A message link (`https://discord.com/channels/g/c/m`), `channel/message`, or `#channel/message`.
fn parse_message_ref<P: DiscordPort>(_app: &App<P>, s: &Snapshot, input: &str) -> Result<(Id, Id), ServiceError> {
    let parts: Vec<&str> = input.trim().trim_end_matches('/').split('/').collect();
    let n = parts.len();
    if n >= 2
        && let Ok(message) = parts[n - 1].parse::<Id>()
    {
        let channel = match parts[n - 2].parse::<Id>() {
            Ok(id) => id,
            Err(_) => resolved(resolve::resolve_channel(s, parts[n - 2]), "channel", parts[n - 2])?,
        };
        return Ok((channel, message));
    }
    Err(invalid(format!("`{input}` isn't a message link or channel/message")))
}

// ---- compose ---------------------------------------------------------------------------------

/// A message to send.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComposeInput {
    pub content: Option<String>,
    pub embeds: Vec<Value>,
    pub attachments: Vec<Asset>,
    pub reply_to: Option<Id>,
    /// A message link or `channel/message` to forward.
    pub forward: Option<String>,
    /// A Components V2 layout: an array of top-level components.
    pub components_v2: Option<Value>,
    pub allow_mentions: Option<MentionsInput>,
}

fn file_name(a: &Asset, i: usize, t: asset::FileType) -> String {
    let from = match a {
        Asset::File { file } => file.rsplit(['/', '\\']).next().map(str::to_owned),
        Asset::Url { url } => url.split('?').next().and_then(|u| u.rsplit('/').next()).map(str::to_owned),
        Asset::Data { .. } => None,
    };
    from.filter(|n| !n.is_empty() && n.contains('.')).unwrap_or_else(|| format!("attachment{}.{}", i + 1, t.ext()))
}

/// Checks a message and builds what to send. Shared by `message_compose`, `webhook_send` and
/// forum posts.
pub(crate) async fn build_message<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    s: &Snapshot,
    input: &ComposeInput,
    caller: Caller<'_>,
) -> Result<OutgoingMessage, ServiceError> {
    let content = input.content.clone().filter(|c| !c.is_empty());
    if content.as_ref().is_some_and(|c| c.chars().count() > MAX_CONTENT) {
        return Err(invalid("content is at most 2000 characters"));
    }
    if input.embeds.len() > MAX_EMBEDS {
        return Err(invalid("at most 10 embeds per message"));
    }
    if input.embeds.iter().any(|e| !e.is_object()) {
        return Err(invalid("each embed is a JSON object (title, description, fields, …)"));
    }
    let mut flags = 0;
    if let Some(c) = &input.components_v2 {
        if content.is_some() || !input.embeds.is_empty() {
            return Err(invalid(
                "a Components V2 message can't also have content or embeds (put text in a text_display component)",
            ));
        }
        if !c.is_array() {
            return Err(invalid("components_v2 is an array of components"));
        }
        let n = count_components(c);
        if n > MAX_COMPONENTS {
            return Err(invalid(format!("a Components V2 message has at most 40 components; this has {n}")));
        }
        flags |= COMPONENTS_V2;
    }
    let forward = input.forward.as_deref().map(|f| parse_message_ref(app, s, f)).transpose()?;
    if forward.is_some() && (content.is_some() || !input.embeds.is_empty() || !input.attachments.is_empty()) {
        return Err(invalid("a forwarded message carries nothing of its own (no content, embeds or attachments)"));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for (i, a) in input.attachments.iter().enumerate() {
        let loaded = assets::load(&app.port, &app.policy, caller, a, AssetKind::Attachment).await?;
        total += loaded.bytes.len() as u64;
        files.push(FileData { name: file_name(a, i, loaded.file_type), bytes: loaded.bytes });
    }
    asset::check_request(total).map_err(|e| invalid(e.to_string()))?;
    let allowed = mentions(app, gid, s, input.allow_mentions.as_ref(), input.reply_to.is_some()).await?;
    let msg = OutgoingMessage {
        content,
        embeds: input.embeds.clone(),
        allowed_mentions: allowed,
        reply_to: input.reply_to,
        forward,
        components: input.components_v2.clone(),
        flags,
        poll: None,
        files,
    };
    let empty = msg.content.is_none()
        && msg.embeds.is_empty()
        && msg.files.is_empty()
        && msg.components.is_none()
        && msg.forward.is_none();
    if empty {
        return Err(invalid("a message needs content, embeds, attachments, components_v2 or forward"));
    }
    Ok(msg)
}

/// What sending needs beyond `message_compose`'s own permissions.
fn send_problems(s: &Snapshot, msg: &OutgoingMessage) -> Vec<PreflightProblem> {
    let mut problems = preflight::check_tools(s, ["message_compose"]);
    let mut need = |p: Permission, why: &str| {
        let admin = s.bot.permissions.contains(Permission::Administrator) || s.bot.user_id == s.guild.owner_id;
        if !admin && !s.bot.permissions.contains(p) {
            problems.push(PreflightProblem::MissingPermission { perm: p.name().to_owned(), needed_by: why.to_owned() });
        }
    };
    if !msg.files.is_empty() {
        need(Permission::AttachFiles, "message_compose with attachments");
    }
    if !msg.embeds.is_empty() {
        need(Permission::EmbedLinks, "message_compose with embeds");
    }
    if msg.allowed_mentions.everyone {
        need(Permission::MentionEveryone, "message_compose mentioning @everyone");
    }
    problems
}

/// Sends a message (embeds, attachments, a reply or forward, Components V2) as the bot.
pub async fn message_compose<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    input: &ComposeInput,
) -> Result<Sent, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let msg = build_message(app, gid, &s, input, Caller::Mcp { base: None }).await?;
    refuse_if(&send_problems(&s, &msg))?;
    let m = app.port.post(cid, &msg).await?;
    Ok(Sent { id: m.id, url: format!("https://discord.com/channels/{gid}/{cid}/{}", m.id) })
}

// ---- edit and delete ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Edited {
    pub id: Id,
    /// What changed; empty when nothing needed to.
    pub changes: Vec<String>,
}

/// Edits a message. Only the bot's own messages can change text or embeds; on others only
/// `suppress_embeds` can be set (with Manage Messages).
pub async fn message_edit<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    content: Option<&str>,
    embeds: Option<&[Value]>,
    suppress_embeds: Option<bool>,
) -> Result<Edited, ServiceError> {
    let (_, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let current = app.port.message(cid, message).await?;
    let own = current.author_id == s.bot.user_id;
    if !own && (content.is_some() || embeds.is_some()) {
        return Err(invalid(
            "only messages the bot wrote can have their text or embeds changed; on other messages only suppress_embeds can be set",
        ));
    }
    if content.is_some_and(|c| c.chars().count() > MAX_CONTENT) {
        return Err(invalid("content is at most 2000 characters"));
    }
    let mut edit = MessageEdit::default();
    let mut changes = Vec::new();
    if let Some(c) = content.filter(|c| *c != current.content) {
        edit.content = Some(c.to_owned());
        changes.push("content".to_owned());
    }
    if let Some(e) = embeds.filter(|e| *e != current.embeds.as_slice()) {
        edit.embeds = Some(e.to_vec());
        changes.push("embeds".to_owned());
    }
    if let Some(sup) = suppress_embeds {
        let flags = if sup { current.flags | SUPPRESS_EMBEDS } else { current.flags & !SUPPRESS_EMBEDS };
        if flags != current.flags {
            edit.flags = Some(flags);
            changes.push(format!("suppress_embeds {}", if sup { "on" } else { "off" }));
        }
    }
    if changes.is_empty() {
        return Ok(Edited { id: message, changes });
    }
    if !own {
        refuse_if(&preflight::check_tools(&s, ["message_delete"]))?;
    }
    app.port.edit(cid, message, &edit).await?;
    Ok(Edited { id: message, changes })
}

fn excerpt(text: &str) -> String {
    let short: String = text.chars().take(60).collect();
    if short.len() < text.len() { format!("{short}…") } else { short }
}

/// Deletes one message after confirmation.
pub async fn message_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (_, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let m = app.port.message(cid, message).await?;
    if m.author_id != s.bot.user_id {
        refuse_if(&preflight::check_tools(&s, ["message_delete"]))?;
    }
    let what = format!("message {message} by {}: \"{}\"", m.author_name, excerpt(&m.content));
    confirm::guarded(confirmed, vec![format!("delete {what}")], || async {
        app.port.delete_message(cid, message, &audit_reason(&format!("delete message {message}"), reason)).await?;
        Ok(Deleted { deleted: format!("message {message}") })
    })
    .await
}

// ---- pins --------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Pinned {
    pub id: Id,
    pub pinned: bool,
    /// `false` when it was already in that state.
    pub changed: bool,
}

async fn pin_count<P: DiscordPort>(app: &App<P>, channel: Id) -> Result<usize, ServiceError> {
    let mut count = 0;
    let mut before: Option<String> = None;
    loop {
        let (page, more) = app.port.pins(channel, before.as_deref(), 50).await?;
        count += page.len();
        before = page.last().map(|(_, at)| at.clone());
        if !more || before.is_none() || count >= MAX_PINS {
            return Ok(count);
        }
    }
}

/// Pins or unpins a message. A channel holds at most 250 pins.
pub async fn message_pin<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    pinned: bool,
    reason: Option<&str>,
) -> Result<Pinned, ServiceError> {
    let (_, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    refuse_if(&preflight::check_tools(&s, ["message_pin"]))?;
    let m = app.port.message(cid, message).await?;
    if m.pinned == pinned {
        return Ok(Pinned { id: message, pinned, changed: false });
    }
    if pinned && pin_count(app, cid).await? >= MAX_PINS {
        return Err(invalid("this channel already has 250 pins, Discord's limit; unpin one first"));
    }
    let verb = if pinned { "pin" } else { "unpin" };
    app.port.set_pinned(cid, message, pinned, &audit_reason(&format!("{verb} message {message}"), reason)).await?;
    Ok(Pinned { id: message, pinned, changed: true })
}

/// A page of pins.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PinsPage {
    pub pins: Vec<MessageView>,
    /// Pass as `before` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before: Option<String>,
}

/// Pinned messages, newest pin first: `limit` ≤ 50 (default 25), `before` a pin time.
pub async fn pins_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    limit: Option<u8>,
    before: Option<&str>,
) -> Result<PinsPage, ServiceError> {
    let (_, s) = server(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let (page, more) = app.port.pins(cid, before, limit.unwrap_or(25).clamp(1, 50)).await?;
    let next_before = more.then(|| page.last().map(|(_, at)| at.clone())).flatten();
    Ok(PinsPage { pins: page.iter().map(|(m, _)| MessageView::from(m)).collect(), next_before })
}

/// Publishes a message from an announcement channel to the channels that follow it.
pub async fn message_crosspost<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
) -> Result<Sent, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    if s.channel(cid).map(|c| c.kind) != Some(LiveChannelKind::Known(ChannelKind::Announcement)) {
        return Err(invalid(format!("`{channel}` isn't an announcement channel; only those can publish")));
    }
    let m = app.port.message(cid, message).await?;
    if m.author_id != s.bot.user_id {
        refuse_if(&preflight::check_tools(&s, ["message_delete"]))?;
    }
    app.port.crosspost(cid, message).await?;
    Ok(Sent { id: message, url: format!("https://discord.com/channels/{gid}/{cid}/{message}") })
}

// ---- reactions ---------------------------------------------------------------------------------

/// What `reactions` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReactionAction {
    Add,
    Remove,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Reacted {
    pub message: Id,
    pub done: String,
}

/// A unicode emoji, `<:name:id>` / `name:id` for a custom one.
fn emoji_arg(e: &str) -> String {
    let e = e.trim().trim_start_matches('<').trim_end_matches('>');
    e.strip_prefix("a:").or_else(|| e.strip_prefix(':')).unwrap_or(e).to_owned()
}

/// Adds the bot's reaction, removes the bot's (or a user's), or clears reactions (all, or one
/// emoji's) after confirmation.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn reactions<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    action: ReactionAction,
    emoji: Option<&str>,
    user: Option<&str>,
    confirmed: bool,
) -> Result<Outcome<Reacted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    let emoji = emoji.map(emoji_arg).filter(|e| !e.is_empty());
    let need_emoji = || emoji.clone().ok_or_else(|| invalid("`emoji` is needed to add or remove a reaction"));
    match action {
        ReactionAction::Add => {
            let e = need_emoji()?;
            refuse_if(&preflight::check_tools(&s, ["reactions"]))?;
            app.port.react(cid, message, &e).await?;
            Ok(Outcome::Done(Reacted { message, done: format!("added {e}") }))
        }
        ReactionAction::Remove => {
            let e = need_emoji()?;
            let who = match user {
                Some(u) => Some(find_member(app, gid, u).await?.id),
                None => None,
            };
            if who.is_some() {
                refuse_if(&preflight::check_tools(&s, ["message_delete"]))?;
            }
            app.port.unreact(cid, message, &e, who).await?;
            Ok(Outcome::Done(Reacted { message, done: format!("removed {e}") }))
        }
        ReactionAction::Clear => {
            refuse_if(&preflight::check_tools(&s, ["message_delete"]))?;
            let what = match &emoji {
                Some(e) => format!("remove every {e} reaction from message {message}"),
                None => format!("remove every reaction from message {message}"),
            };
            confirm::guarded(confirmed, vec![what.clone()], || async {
                app.port.clear_reactions(cid, message, emoji.as_deref()).await?;
                Ok(Reacted { message, done: what.clone() })
            })
            .await
        }
    }
}

/// Who reacted with one emoji, in pages: `limit` ≤ 100 (default 50), `after` a user id.
pub async fn reactions_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    message: Id,
    emoji: &str,
    limit: Option<u8>,
    after: Option<Id>,
) -> Result<Vec<UserInfo>, ServiceError> {
    let (_, s) = server(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    Ok(app.port.reaction_users(cid, message, &emoji_arg(emoji), after, limit.unwrap_or(50).clamp(1, 100)).await?)
}

// ---- search ----------------------------------------------------------------------------------

/// A search request (FR-020, research E11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchInput {
    pub content: Option<String>,
    pub author: Option<String>,
    pub channels: Vec<String>,
    pub has: Vec<String>,
    /// A message id or RFC 3339 time.
    pub before: Option<String>,
    pub after: Option<String>,
    /// Scan recent history (up to 500 messages per channel) instead of Discord's search.
    pub scan: bool,
    pub limit: Option<u8>,
    pub offset: Option<u32>,
}

/// What a search found. `messages` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SearchResult {
    pub messages: Vec<MessageView>,
    pub total: u64,
    /// `false` while Discord is still indexing the server; try again after `retry_after`.
    pub indexed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// Pass as `offset` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

const HAS: [&str; 9] = ["link", "embed", "file", "image", "video", "sound", "sticker", "poll", "snapshot"];

/// A snowflake for a message id or a time (the smallest id at that millisecond).
fn snowflake(input: &str) -> Result<Id, ServiceError> {
    if let Ok(id) = input.trim().parse::<Id>() {
        return Ok(id);
    }
    let secs = date::parse_rfc3339(input)
        .ok_or_else(|| invalid(format!("`{input}` is neither a message id nor an RFC 3339 time with an offset")))?;
    let ms = secs * 1000 - DISCORD_EPOCH_MS;
    u64::try_from(ms).map(|ms| Id(ms << 22)).map_err(|_| invalid(format!("`{input}` is before Discord existed")))
}

fn matches_has(m: &MessageInfo, has: &[String]) -> bool {
    has.iter().all(|h| match h.as_str() {
        "file" | "image" | "video" | "sound" => !m.attachments.is_empty(),
        "embed" => !m.embeds.is_empty(),
        "poll" => m.poll.is_some(),
        "link" => m.content.contains("http://") || m.content.contains("https://"),
        _ => true,
    })
}

/// Searches messages through Discord's guild search, or with `scan` by reading up to 500 recent
/// messages per channel. `limit` ≤ 25 (default 25), `offset` ≤ 9975.
pub async fn messages_search<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    input: &SearchInput,
) -> Result<SearchResult, ServiceError> {
    let limit = input.limit.unwrap_or(25).clamp(1, 25);
    let offset = input.offset.unwrap_or(0);
    if offset > 9975 {
        return Err(invalid("offset is at most 9975"));
    }
    if let Some(h) = input.has.iter().find(|h| !HAS.contains(&h.as_str())) {
        return Err(invalid(format!("unknown `has` value `{h}`; use {}", HAS.join(", "))));
    }
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["messages_search"]))?;
    let mut channels = Vec::new();
    for c in &input.channels {
        channels.push(message_channel(app, &s, c).await?);
    }
    let author = match &input.author {
        Some(a) => Some(match a.trim().trim_start_matches("<@").trim_end_matches('>').parse::<Id>() {
            Ok(id) => id,
            Err(_) => find_member(app, gid, a).await?.id,
        }),
        None => None,
    };
    let (min_id, max_id) =
        (input.after.as_deref().map(snowflake).transpose()?, input.before.as_deref().map(snowflake).transpose()?);
    let mut notes = Vec::new();
    let content_on = app.port.application_flags().await.map_or(true, |f| f & preflight::MESSAGE_CONTENT_FLAGS != 0);
    if !content_on {
        notes.push(
            "the bot's MESSAGE_CONTENT intent is off, so message text may be empty; turn it on in the developer portal"
                .to_owned(),
        );
    }
    if input.scan {
        return scan(app, &s, input, channels, author, (min_id, max_id), (limit, offset), notes).await;
    }
    let q = SearchQuery {
        content: input.content.clone().filter(|c| !c.is_empty()),
        authors: author.into_iter().collect(),
        channels,
        has: input.has.clone(),
        min_id,
        max_id,
        limit,
        offset,
    };
    match app.port.search(gid, &q).await {
        Ok(SearchOutcome::NotIndexed { retry_after }) => {
            notes.push(format!("Discord is still indexing this server; try again in {retry_after} s, or use scan"));
            Ok(SearchResult {
                messages: Vec::new(),
                total: 0,
                indexed: false,
                retry_after: Some(retry_after),
                next_offset: None,
                notes,
            })
        }
        Ok(SearchOutcome::Found { total, messages }) => {
            let next = u64::from(offset) + messages.len() as u64;
            let next_offset = (next < total && next <= 9975).then(|| u32::try_from(next).unwrap_or(9975));
            Ok(SearchResult {
                messages: messages.iter().map(MessageView::from).collect(),
                total,
                indexed: true,
                retry_after: None,
                next_offset,
                notes,
            })
        }
        Err(PortError::Forbidden(m)) => Err(ServiceError::Port(PortError::Forbidden(format!(
            "{m}; Discord's search needs the MESSAGE_CONTENT intent and Read Message History. Use scan: true to read recent history instead"
        )))),
        Err(e) => Err(e.into()),
    }
}

#[allow(clippy::too_many_arguments)] // the parsed search, passed through
async fn scan<P: DiscordPort>(
    app: &App<P>,
    s: &Snapshot,
    input: &SearchInput,
    mut channels: Vec<Id>,
    author: Option<Id>,
    (min_id, max_id): (Option<Id>, Option<Id>),
    (limit, offset): (u8, u32),
    mut notes: Vec<String>,
) -> Result<SearchResult, ServiceError> {
    if channels.is_empty() {
        channels = s
            .channels
            .iter()
            .filter(|c| matches!(c.kind, LiveChannelKind::Known(k) if k.is_text_like() && !k.is_forum_like()))
            .map(|c| c.id)
            .take(10)
            .collect();
        notes.push(format!("scanned the first {} text channel(s); list channels to choose", channels.len()));
    }
    let needle = input.content.as_deref().map(str::to_lowercase).filter(|c| !c.is_empty());
    let mut found: Vec<MessageInfo> = Vec::new();
    for c in channels {
        let mut before = max_id;
        let mut read = 0;
        while read < SCAN_LIMIT {
            let page = app.port.recent(c, 100, before).await?;
            let Some(last) = page.last() else { break };
            before = Some(last.id);
            read += page.len();
            let done = min_id.is_some_and(|m| last.id <= m);
            found.extend(page.into_iter().filter(|m| {
                min_id.is_none_or(|i| m.id > i)
                    && author.is_none_or(|a| m.author_id == a)
                    && needle.as_ref().is_none_or(|n| m.content.to_lowercase().contains(n))
                    && matches_has(m, &input.has)
            }));
            if done {
                break;
            }
        }
    }
    found.sort_by_key(|m| std::cmp::Reverse(m.id));
    let total = found.len() as u64;
    let page: Vec<MessageView> =
        found.iter().skip(offset as usize).take(usize::from(limit)).map(MessageView::from).collect();
    let next = u64::from(offset) + page.len() as u64;
    notes.push(format!("scan: read at most {SCAN_LIMIT} recent messages per channel"));
    Ok(SearchResult {
        total,
        next_offset: (next < total).then(|| u32::try_from(next).unwrap_or(u32::MAX)),
        messages: page,
        indexed: true,
        retry_after: None,
        notes,
    })
}

/// `build_message` with the lab defaults, for tests.
#[cfg(feature = "fake")]
pub async fn build_message_for_test<P: DiscordPort>(
    app: &App<P>,
    input: &ComposeInput,
) -> Result<OutgoingMessage, ServiceError> {
    let (gid, s) = server(app, None).await?;
    build_message(app, gid, &s, input, Caller::Mcp { base: None }).await
}
