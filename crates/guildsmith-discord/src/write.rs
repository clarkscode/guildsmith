//! The write path. Every write sets the audit-log reason, and none is ever retried (FR-013).
//!
//! Channel creates and updates are sent as raw JSON so one request carries the fields, the
//! parent and the full `permission_overwrites` list (research R7), with explicit nulls where a
//! value is cleared, and forum tags that don't have an id yet.

use guildsmith_app::ports::{ChannelFields, ChannelPosition, PortError, PortResult, RoleFields, RolePosition};
use guildsmith_core::Id;
use guildsmith_core::snapshot::{LiveOverwrite, OverwriteKind};
use guildsmith_core::spec::ForumTag;
use http::header::{HeaderName, HeaderValue};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use twilight_http::Client;
use twilight_http::request::{AuditLogReason, Request};
use twilight_http::routing::Route;
use twilight_model::channel::Channel;
use twilight_model::guild::RolePosition as TRolePosition;
use twilight_model::http::channel_position::Position;

use crate::error::map_err;
use crate::{ids, map};

/// `X-Audit-Log-Reason`, percent-encoded as Discord requires.
fn audit(reason: &str) -> PortResult<impl Iterator<Item = (HeaderName, HeaderValue)>> {
    let value = HeaderValue::from_str(&utf8_percent_encode(reason, NON_ALPHANUMERIC).to_string())
        .map_err(|e| PortError::Discord { status: 0, code: None, message: format!("bad audit reason: {e}") })?;
    Ok(std::iter::once((HeaderName::from_static("x-audit-log-reason"), value)))
}

pub(crate) async fn create_role(http: &Client, guild: Id, f: &RoleFields, reason: &str) -> PortResult<Id> {
    let mut req = http.create_role(ids::guild(guild)?);
    if let Some(v) = &f.name {
        req = req.name(v);
    }
    if let Some(v) = f.color {
        req = req.color(v);
    }
    if let Some(v) = f.hoist {
        req = req.hoist(v);
    }
    if let Some(v) = f.mentionable {
        req = req.mentionable(v);
    }
    if let Some(v) = f.permissions {
        req = req.permissions(map::to_perms(v));
    }
    let role = req.reason(reason).await.map_err(map_err)?.model().await.map_err(map_err)?;
    Ok(ids::from(role.id))
}

pub(crate) async fn update_role(http: &Client, guild: Id, role: Id, f: &RoleFields, reason: &str) -> PortResult<()> {
    let mut req = http.update_role(ids::guild(guild)?, ids::role(role)?);
    if let Some(v) = &f.name {
        req = req.name(Some(v));
    }
    if let Some(v) = f.color {
        req = req.color(Some(v));
    }
    if let Some(v) = f.hoist {
        req = req.hoist(v);
    }
    if let Some(v) = f.mentionable {
        req = req.mentionable(v);
    }
    if let Some(v) = f.permissions {
        req = req.permissions(map::to_perms(v));
    }
    req.reason(reason).await.map_err(map_err)?;
    Ok(())
}

pub(crate) async fn delete_role(http: &Client, guild: Id, role: Id, reason: &str) -> PortResult<()> {
    http.delete_role(ids::guild(guild)?, ids::role(role)?).reason(reason).await.map_err(map_err)?;
    Ok(())
}

pub(crate) async fn reorder_roles(http: &Client, guild: Id, order: &[RolePosition], reason: &str) -> PortResult<()> {
    let list = order
        .iter()
        .map(|p| {
            let position = u64::try_from(p.position).map_err(|_| PortError::Discord {
                status: 0,
                code: None,
                message: "negative role position".into(),
            })?;
            Ok(TRolePosition { id: ids::role(p.id)?, position })
        })
        .collect::<PortResult<Vec<_>>>()?;
    http.update_role_positions(ids::guild(guild)?, &list).reason(reason).await.map_err(map_err)?;
    Ok(())
}

/// A permission overwrite in Discord's JSON form.
#[derive(Serialize)]
struct OverwriteBody {
    id: String,
    #[serde(rename = "type")]
    kind: u8,
    allow: String,
    deny: String,
}

fn overwrite_body(o: &LiveOverwrite) -> OverwriteBody {
    OverwriteBody {
        id: o.id.to_string(),
        kind: match o.kind {
            OverwriteKind::Role => 0,
            OverwriteKind::Member => 1,
        },
        allow: o.allow.to_discord_string(),
        deny: o.deny.to_discord_string(),
    }
}

/// An emoji given as a custom emoji id (digits) or a unicode emoji.
#[derive(Serialize)]
struct EmojiBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji_name: Option<String>,
}

fn emoji(e: &str) -> EmojiBody {
    if !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit()) {
        EmojiBody { emoji_id: Some(e.to_owned()), emoji_name: None }
    } else {
        EmojiBody { emoji_id: None, emoji_name: Some(e.to_owned()) }
    }
}

#[derive(Serialize)]
struct TagBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    moderated: bool,
    #[serde(flatten)]
    emoji: Option<EmojiBody>,
}

fn tag_body(t: &ForumTag) -> TagBody {
    TagBody {
        id: t.id.map(|i| i.to_string()),
        name: t.name.clone(),
        moderated: t.moderated.unwrap_or(false),
        emoji: t.emoji.as_deref().map(emoji),
    }
}

/// The JSON body for Create Guild Channel / Modify Channel. Absent fields are left alone;
/// `Some(None)` sends `null`, which clears the value.
#[derive(Serialize, Default)]
#[allow(clippy::option_option)] // three states: leave alone, clear (null), set
struct ChannelBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_per_user: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bitrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtc_region: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_tags: Option<Vec<TagBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_reaction_emoji: Option<Option<EmojiBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_overwrites: Option<Vec<OverwriteBody>>,
}

fn channel_body(f: &ChannelFields) -> ChannelBody {
    ChannelBody {
        name: f.name.clone(),
        kind: f.kind.map(|k| u8::from(map::channel::to_twilight_kind(k))),
        parent_id: f.parent_id.map(|p| p.map(|id| id.to_string())),
        position: f.position,
        topic: f.topic.as_ref().map(|t| (!t.is_empty()).then(|| t.clone())),
        nsfw: f.nsfw,
        rate_limit_per_user: f.slowmode,
        bitrate: f.bitrate,
        user_limit: f.user_limit,
        rtc_region: f.rtc_region.clone(),
        available_tags: f.tags.as_ref().map(|t| t.iter().map(tag_body).collect()),
        default_reaction_emoji: f.default_reaction.as_ref().map(|r| r.as_deref().map(emoji)),
        permission_overwrites: f.overwrites.as_ref().map(|o| o.iter().map(overwrite_body).collect()),
    }
}

pub(crate) async fn create_channel(http: &Client, guild: Id, f: &ChannelFields, reason: &str) -> PortResult<Id> {
    let mut body = channel_body(f);
    // On create, "no parent" is simply leaving it out.
    if body.parent_id == Some(None) {
        body.parent_id = None;
    }
    let req = Request::builder(&Route::CreateChannel { guild_id: guild.get() })
        .json(&body)
        .headers(audit(reason)?)
        .build()
        .map_err(map_err)?;
    let ch = http.request::<Channel>(req).await.map_err(map_err)?.model().await.map_err(map_err)?;
    Ok(ids::from(ch.id))
}

pub(crate) async fn update_channel(http: &Client, channel: Id, f: &ChannelFields, reason: &str) -> PortResult<()> {
    let req = Request::builder(&Route::UpdateChannel { channel_id: channel.get() })
        .json(&channel_body(f))
        .headers(audit(reason)?)
        .build()
        .map_err(map_err)?;
    http.request::<Channel>(req).await.map_err(map_err)?;
    Ok(())
}

pub(crate) async fn delete_channel(http: &Client, channel: Id, reason: &str) -> PortResult<()> {
    http.delete_channel(ids::channel(channel)?).reason(reason).await.map_err(map_err)?;
    Ok(())
}

/// Positions only: guildsmith moves channels between categories in each channel's own update,
/// because Discord allows one `parent_id` change per bulk request (research R8).
pub(crate) async fn reorder_channels(
    http: &Client,
    guild: Id,
    positions: &[ChannelPosition],
    reason: &str,
) -> PortResult<()> {
    let list = positions
        .iter()
        .map(|p| {
            Ok(Position {
                id: ids::channel(p.id)?,
                lock_permissions: None,
                parent_id: p.parent_id.map(|parent| parent.map(ids::channel).transpose()).transpose()?,
                position: Some(Some(u64::try_from(p.position).unwrap_or(0))),
            })
        })
        .collect::<PortResult<Vec<_>>>()?;
    // The bulk positions endpoint takes no audit reason header in twilight; send it raw.
    let req = Request::builder(&Route::UpdateGuildChannels { guild_id: guild.get() })
        .json(&list)
        .headers(audit(reason)?)
        .build()
        .map_err(map_err)?;
    http.request::<twilight_http::response::marker::EmptyBody>(req).await.map_err(map_err)?;
    Ok(())
}
