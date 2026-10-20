//! `WebhooksPort` for the real Discord API. Tokens are read into [`Secret`]s and never put in
//! a `WebhookInfo`; posting through a webhook uses its token route without the bot token.

use std::fmt::Write as _;

use guildsmith_app::ports::{
    MessageInfo, OutgoingMessage, PortError, PortResult, WebhookFields, WebhookInfo, WebhookKind, WebhooksPort,
};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::request::Method;

use crate::messages::{RawMessage, RawUser, message_json};
use crate::port::TwilightDiscord;
use crate::raw;

#[derive(Debug, Deserialize)]
struct RawSource {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawWebhook {
    id: Id,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    channel_id: Option<Id>,
    #[serde(default)]
    user: Option<RawUser>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    source_channel: Option<RawSource>,
    #[serde(default)]
    avatar: Option<String>,
}

impl RawWebhook {
    /// The webhook's details, and its token separately (never in the info).
    fn split(self) -> (WebhookInfo, Option<Secret>) {
        let kind = match self.kind {
            2 => WebhookKind::ChannelFollower,
            3 => WebhookKind::Application,
            _ => WebhookKind::Incoming,
        };
        let info = WebhookInfo {
            id: self.id,
            kind,
            name: self.name.unwrap_or_default(),
            channel_id: self.channel_id.unwrap_or(Id(0)),
            creator_id: self.user.as_ref().map(|u| u.id),
            creator_name: self.user.map(|u| u.global_name.unwrap_or(u.username)),
            source: self.source_channel.and_then(|s| s.name),
            avatar: self.avatar,
        };
        (info, self.token.map(Secret::new))
    }
}

fn parse_list(v: Value) -> PortResult<Vec<WebhookInfo>> {
    let list: Vec<RawWebhook> = serde_json::from_value(v).map_err(|e| PortError::Discord {
        status: 0,
        code: None,
        message: format!("unexpected webhook list: {e}"),
    })?;
    Ok(list.into_iter().map(|w| w.split().0).collect())
}

fn parse_one(res: &raw::Response, what: &str) -> PortResult<(WebhookInfo, Option<Secret>)> {
    Ok(raw::parse::<RawWebhook>(res, what)?.split())
}

impl WebhooksPort for TwilightDiscord {
    async fn channel_webhooks(&self, channel: Id) -> PortResult<Vec<WebhookInfo>> {
        let http = self.http().await?;
        let res = raw::send(http, Method::Get, &format!("channels/{channel}/webhooks"), &[], None, None).await?;
        parse_list(res.json()?)
    }

    async fn guild_webhooks(&self, guild: Id) -> PortResult<Vec<WebhookInfo>> {
        let http = self.http().await?;
        let res = raw::send(http, Method::Get, &format!("guilds/{guild}/webhooks"), &[], None, None).await?;
        parse_list(res.json()?)
    }

    async fn create_webhook(
        &self,
        channel: Id,
        name: &str,
        avatar: Option<&str>,
        reason: &str,
    ) -> PortResult<(WebhookInfo, Secret)> {
        let http = self.http().await?;
        let mut body = json!({"name": name});
        if let Some(a) = avatar {
            body["avatar"] = json!(a);
        }
        let res =
            raw::send(http, Method::Post, &format!("channels/{channel}/webhooks"), &[], Some(&body), Some(reason))
                .await?;
        let (info, token) = parse_one(&res, "create webhook")?;
        let token = token.ok_or_else(|| PortError::Discord {
            status: res.status,
            code: None,
            message: "Discord created the webhook but returned no token".to_owned(),
        })?;
        Ok((info, token))
    }

    async fn update_webhook(&self, webhook: Id, f: &WebhookFields, reason: &str) -> PortResult<WebhookInfo> {
        let http = self.http().await?;
        let mut b = Map::new();
        if let Some(n) = &f.name {
            b.insert("name".to_owned(), json!(n));
        }
        if let Some(a) = &f.avatar {
            b.insert("avatar".to_owned(), json!(a));
        }
        if let Some(c) = f.channel_id {
            b.insert("channel_id".to_owned(), json!(c));
        }
        let res =
            raw::send(http, Method::Patch, &format!("webhooks/{webhook}"), &[], Some(&Value::Object(b)), Some(reason))
                .await?;
        Ok(parse_one(&res, "update webhook")?.0)
    }

    async fn delete_webhook(&self, webhook: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("webhooks/{webhook}"), &[], None, Some(reason)).await?;
        Ok(())
    }

    async fn webhook_token(&self, webhook: Id) -> PortResult<Option<Secret>> {
        let http = self.http().await?;
        let res = raw::send(http, Method::Get, &format!("webhooks/{webhook}"), &[], None, None).await?;
        Ok(parse_one(&res, "get webhook")?.1)
    }

    async fn execute_webhook(
        &self,
        webhook: Id,
        token: &Secret,
        msg: &OutgoingMessage,
        thread: Option<Id>,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> PortResult<MessageInfo> {
        let http = self.http().await?;
        let mut body = message_json(msg);
        if let Some(u) = username {
            body["username"] = json!(u);
        }
        if let Some(a) = avatar_url {
            body["avatar_url"] = json!(a);
        }
        let mut target = format!("webhooks/{webhook}/{}?wait=true", token.expose());
        if let Some(t) = thread {
            let _ = write!(target, "&thread_id={t}");
        }
        let files: Vec<(String, Vec<u8>)> = msg.files.iter().map(|f| (f.name.clone(), f.bytes.clone())).collect();
        let res = raw::send_webhook(http, target, &body, &files).await?;
        Ok(raw::parse::<RawMessage>(&res, "webhook message")?.into())
    }
}
