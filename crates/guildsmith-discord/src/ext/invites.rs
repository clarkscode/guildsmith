//! Invite `role_ids` and the target-user endpoints (`invites/{code}/target-users`, bulk add and
//! delete of at most 1000 users), missing in twilight 0.17.1. The target-user response shape is
//! read leniently (a list of users, or `{users: [...]}`) until it is checked live.

use guildsmith_app::ports::{InviteInfo, NewInvite, PortResult};
use guildsmith_core::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use twilight_http::Client;
use twilight_http::request::Method;

use crate::messages::RawUser;
use crate::raw;

#[derive(Debug, Deserialize)]
struct RawChannelRef {
    id: Id,
}

#[derive(Debug, Deserialize)]
struct RawRole {
    id: Id,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawInvite {
    code: String,
    #[serde(default)]
    channel: Option<RawChannelRef>,
    #[serde(default)]
    inviter: Option<RawUser>,
    #[serde(default)]
    max_age: u32,
    #[serde(default)]
    max_uses: u32,
    #[serde(default)]
    uses: u32,
    #[serde(default)]
    temporary: bool,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    role_ids: Option<Vec<Id>>,
    #[serde(default)]
    roles: Option<Vec<RawRole>>,
}

impl From<RawInvite> for InviteInfo {
    fn from(i: RawInvite) -> Self {
        let roles = i.role_ids.unwrap_or_else(|| i.roles.unwrap_or_default().into_iter().map(|r| r.id).collect());
        InviteInfo {
            code: i.code,
            channel_id: i.channel.map_or(Id(0), |c| c.id),
            inviter_id: i.inviter.as_ref().map(|u| u.id),
            inviter: i.inviter.map(|u| u.global_name.unwrap_or(u.username)),
            max_age: i.max_age,
            max_uses: i.max_uses,
            uses: i.uses,
            temporary: i.temporary,
            created_at: i.created_at,
            expires_at: i.expires_at,
            roles,
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateBody<'a> {
    max_age: u32,
    max_uses: u32,
    temporary: bool,
    unique: bool,
    #[serde(skip_serializing_if = "<[Id]>::is_empty")]
    role_ids: &'a [Id],
}

pub(crate) async fn create(http: &Client, channel: Id, i: &NewInvite, reason: &str) -> PortResult<InviteInfo> {
    let body = CreateBody {
        max_age: i.max_age,
        max_uses: i.max_uses,
        temporary: i.temporary,
        unique: i.unique,
        role_ids: &i.roles,
    };
    let raw: RawInvite =
        raw::json(http, Method::Post, &format!("channels/{channel}/invites"), &[], Some(&body), Some(reason)).await?;
    Ok(raw.into())
}

pub(crate) async fn target_users(http: &Client, code: &str) -> PortResult<Vec<Id>> {
    let v: Value =
        raw::json::<(), _>(http, Method::Get, &format!("invites/{code}/target-users"), &[], None, None).await?;
    let list = v.get("users").cloned().unwrap_or(v);
    Ok(list
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|u| u.get("id").or(Some(u)).and_then(|x| x.as_str()).and_then(|x| x.parse().ok()))
                .collect()
        })
        .unwrap_or_default())
}

#[derive(Debug, Serialize)]
struct Targets<'a> {
    user_ids: &'a [Id],
}

/// Adds (`true`) or removes target users, at most 1000 per request.
pub(crate) async fn set_targets(http: &Client, code: &str, users: &[Id], add: bool) -> PortResult<()> {
    let method = if add { Method::Post } else { Method::Delete };
    let path = format!("invites/{code}/target-users");
    raw::send(
        http,
        method,
        &path,
        &[],
        Some(&serde_json::to_value(Targets { user_ids: users }).unwrap_or_default()),
        None,
    )
    .await?;
    Ok(())
}
