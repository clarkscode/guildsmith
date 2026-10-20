//! The `webhooks` group (US5, FR-029/030): create, list, rename, move, delete and post through
//! webhooks.
//!
//! A webhook URL is a credential. It appears only in `webhook_create`'s result (as
//! `secrets.url`), never in lists, logs or errors. guildsmith posts through a webhook with a
//! token it holds in memory (webhooks created in this process) or one Discord returns.

use guildsmith_core::Id;
use guildsmith_core::asset::{Asset, AssetKind, data_uri};
use guildsmith_core::snapshot::Snapshot;
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::assets::{self, Caller};
use crate::confirm::{self, Outcome};
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, WebhookFields, WebhookInfo, WebhookKind};
use crate::preflight;
use crate::resolve;
use crate::secrets::webhook_url;
use crate::services::live::{Deleted, Sent, invalid, refuse_if, resolved, server, writable};
use crate::services::messages::{ComposeInput, build_message, message_channel};

pub const MAX_PER_CHANNEL: usize = 15;
pub const MAX_PER_SERVER: usize = 1000;

/// A webhook as tools return it: no token, no URL. `name` is user-chosen: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WebhookRow {
    pub id: Id,
    pub name: String,
    pub channel: String,
    pub kind: WebhookKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    /// Follower webhooks: the channel they follow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
}

fn row(s: &Snapshot, w: &WebhookInfo) -> WebhookRow {
    WebhookRow {
        id: w.id,
        name: w.name.clone(),
        channel: s.channel(w.channel_id).map_or_else(|| w.channel_id.to_string(), |c| format!("#{}", c.name)),
        kind: w.kind,
        creator: w.creator_name.clone(),
        follows: w.source.clone(),
    }
}

/// Names are 1–80 characters and can't contain "clyde" or "discord".
fn check_name(name: &str) -> Result<(), ServiceError> {
    let lower = name.to_lowercase();
    if !(1..=80).contains(&name.trim().chars().count()) {
        return Err(invalid("webhook names are 1–80 characters"));
    }
    if lower.contains("clyde") || lower.contains("discord") {
        return Err(invalid("webhook names can't contain \"clyde\" or \"discord\""));
    }
    Ok(())
}

async fn avatar<P: DiscordPort>(app: &App<P>, a: Option<&Asset>) -> Result<Option<String>, ServiceError> {
    match a {
        Some(a) => {
            let l = assets::load(&app.port, &app.policy, Caller::Mcp { base: None }, a, AssetKind::Avatar).await?;
            Ok(Some(data_uri(l.file_type, &l.bytes)))
        }
        None => Ok(None),
    }
}

/// The URL, shown once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WebhookSecret {
    /// Secret: anyone with this URL can post as the webhook. Copy it now; it isn't shown again.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WebhookCreated {
    pub webhook: WebhookRow,
    pub secrets: WebhookSecret,
}

/// Creates an incoming webhook in a channel (≤ 15 per channel, ≤ 1000 per server) and returns
/// its URL, once.
pub async fn webhook_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    name: &str,
    avatar_asset: Option<&Asset>,
    reason: Option<&str>,
) -> Result<WebhookCreated, ServiceError> {
    check_name(name)?;
    let (gid, s) = writable(app, guild).await?;
    let cid = message_channel(app, &s, channel).await?;
    refuse_if(&preflight::check_tools(&s, ["webhook_create"]))?;
    if app.port.channel_webhooks(cid).await?.len() >= MAX_PER_CHANNEL {
        return Err(invalid("this channel already has 15 webhooks, Discord's limit"));
    }
    if app.port.guild_webhooks(gid).await?.len() >= MAX_PER_SERVER {
        return Err(invalid("this server already has 1000 webhooks, Discord's limit"));
    }
    let image = avatar(app, avatar_asset).await?;
    let (info, token) = app
        .port
        .create_webhook(
            cid,
            name.trim(),
            image.as_deref(),
            &audit_reason(&format!("create webhook {}", name.trim()), reason),
        )
        .await?;
    let url = webhook_url(info.id, &token);
    app.webhook_tokens.insert(info.id, token);
    Ok(WebhookCreated { webhook: row(&s, &info), secrets: WebhookSecret { url } })
}

/// Webhooks in the server, or one channel, with their kind and creator. Never tokens or URLs.
pub async fn webhooks_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: Option<&str>,
) -> Result<Vec<WebhookRow>, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["webhooks_list"]))?;
    let all = match channel {
        Some(c) => app.port.channel_webhooks(resolved(resolve::resolve_channel(&s, c), "channel", c)?).await?,
        None => app.port.guild_webhooks(gid).await?,
    };
    Ok(all.iter().map(|w| row(&s, w)).collect())
}

async fn find<P: DiscordPort>(app: &App<P>, gid: Id, input: &str) -> Result<WebhookInfo, ServiceError> {
    let all = app.port.guild_webhooks(gid).await?;
    let raw = input.trim();
    if let Ok(id) = raw.parse::<Id>()
        && let Some(w) = all.iter().find(|w| w.id == id)
    {
        return Ok(w.clone());
    }
    let hits: Vec<&WebhookInfo> = all.iter().filter(|w| w.name == raw).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(ServiceError::NotFound {
            what: "webhook",
            input: input.to_owned(),
            suggestions: guildsmith_core::suggest::closest(raw, all.iter().map(|w| w.name.as_str()), 3),
        }),
        many => Err(ServiceError::Ambiguous {
            what: "webhook",
            input: input.to_owned(),
            candidates: many
                .iter()
                .map(|w| resolve::Candidate { name: w.name.clone(), id: w.id, parent: None })
                .collect(),
        }),
    }
}

fn incoming_only(w: &WebhookInfo) -> Result<(), ServiceError> {
    match w.kind {
        WebhookKind::Incoming => Ok(()),
        WebhookKind::ChannelFollower => Err(invalid(format!(
            "{} follows an announcement channel; manage it from that channel's Follow settings",
            w.name
        ))),
        WebhookKind::Application => Err(invalid(format!("{} belongs to an application and is read-only here", w.name))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WebhookUpdated {
    pub webhook: WebhookRow,
    /// What changed; empty when nothing needed to.
    pub changes: Vec<String>,
}

/// Renames a webhook, changes its avatar, or moves it to another channel. Only incoming
/// webhooks can change.
pub async fn webhook_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    webhook: &str,
    name: Option<&str>,
    avatar_asset: Option<&Asset>,
    channel: Option<&str>,
    reason: Option<&str>,
) -> Result<WebhookUpdated, ServiceError> {
    if let Some(n) = name {
        check_name(n)?;
    }
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["webhook_update"]))?;
    let w = find(app, gid, webhook).await?;
    incoming_only(&w)?;
    let mut f = WebhookFields::default();
    let mut changes = Vec::new();
    if let Some(n) = name.map(str::trim).filter(|n| *n != w.name) {
        changes.push(format!("name {:?} → {n:?}", w.name));
        f.name = Some(n.to_owned());
    }
    if let Some(c) = channel {
        let cid = message_channel(app, &s, c).await?;
        if cid != w.channel_id {
            changes.push(format!("channel → {c}"));
            f.channel_id = Some(cid);
        }
    }
    if let Some(a) = avatar(app, avatar_asset).await? {
        changes.push("avatar".to_owned());
        f.avatar = Some(Some(a));
    }
    if changes.is_empty() {
        return Ok(WebhookUpdated { webhook: row(&s, &w), changes });
    }
    let summary = format!("update webhook {}: {}", w.name, changes.join(", "));
    let updated = app.port.update_webhook(w.id, &f, &audit_reason(&summary, reason)).await?;
    Ok(WebhookUpdated { webhook: row(&s, &updated), changes })
}

/// Deletes a webhook after confirmation; its URL stops working.
pub async fn webhook_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    webhook: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["webhook_delete"]))?;
    let w = find(app, gid, webhook).await?;
    incoming_only(&w)?;
    let what = format!("webhook {} ({}) in {}", w.name, w.id, row(&s, &w).channel);
    confirm::guarded(confirmed, vec![format!("delete {what}; anything posting through it stops working")], || async {
        app.port.delete_webhook(w.id, &audit_reason(&format!("delete webhook {}", w.id), reason)).await?;
        app.webhook_tokens.remove(w.id);
        Ok(Deleted { deleted: format!("webhook {}", w.id) })
    })
    .await
}

/// Posts through a webhook, optionally into one of its channel's threads and under another
/// name or avatar. Needs the webhook's token: from this process, or from Discord.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn webhook_send<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    webhook: &str,
    input: &ComposeInput,
    thread: Option<&str>,
    username: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<Sent, ServiceError> {
    if let Some(u) = username {
        check_name(u)?;
    }
    let (gid, s) = writable(app, guild).await?;
    let w = find(app, gid, webhook).await?;
    incoming_only(&w)?;
    let token = match app.webhook_tokens.get(w.id) {
        Some(t) => t,
        None => app.port.webhook_token(w.id).await?.ok_or_else(|| {
            invalid(format!(
                "guildsmith doesn't have {}'s token: it can post only through webhooks it created in this session, \
                 or whose token Discord returns",
                w.name
            ))
        })?,
    };
    let thread_id = match thread {
        Some(t) => Some(message_channel(app, &s, t).await?),
        None => None,
    };
    let msg = build_message(app, gid, &s, input, Caller::Mcp { base: None }).await?;
    let m = app.port.execute_webhook(w.id, &token, &msg, thread_id, username, avatar_url).await?;
    let channel = thread_id.unwrap_or(w.channel_id);
    Ok(Sent { id: m.id, url: format!("https://discord.com/channels/{gid}/{channel}/{}", m.id) })
}
