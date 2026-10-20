//! `WebhooksPort` for the fake: names 1–80 without "clyde" or "discord", at most 15 per
//! channel and 1000 per server, tokens checked on execute, and a switch for whether
//! `GET webhooks/{id}` returns tokens (unverified, research T063).

use std::collections::BTreeMap;

use guildsmith_core::{Id, Permission};

use super::assets::uploaded;
use super::{FakeDiscord, next_id, not_found};
use crate::ports::{
    MessageInfo, OutgoingMessage, PortError, PortResult, WebhookFields, WebhookInfo, WebhookKind, WebhooksPort,
};
use crate::secret::Secret;

/// A webhook held by the fake.
#[derive(Debug, Clone)]
pub struct FakeWebhook {
    pub info: WebhookInfo,
    pub token: Option<String>,
    pub avatar: Option<String>,
}

/// The fake's webhooks for one server.
#[derive(Debug, Clone, Default)]
pub struct WebhooksState {
    pub hooks: BTreeMap<Id, FakeWebhook>,
    /// Whether `GET webhooks/{id}` returns the token.
    pub return_tokens: bool,
}

fn bad(code: u64, message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(code), message: message.to_owned() }
}

fn check_name(name: &str) -> PortResult<()> {
    let lower = name.to_lowercase();
    if !(1..=80).contains(&name.chars().count()) || lower.contains("clyde") || lower.contains("discord") {
        return Err(bad(50035, "Invalid webhook name"));
    }
    Ok(())
}

impl FakeDiscord {
    fn guild_of_webhook(&self, id: Id) -> PortResult<Id> {
        self.lock()
            .guilds
            .values()
            .find(|g| g.res.peek::<WebhooksState>().is_some_and(|s| s.hooks.contains_key(&id)))
            .map(|g| g.snapshot.guild.id)
            .ok_or_else(|| not_found("webhook", id))
    }

    /// Adds a webhook, as if someone made it in Discord. Returns its id.
    pub fn seed_webhook(&self, guild: Id, channel: Id, name: &str, kind: WebhookKind, token: Option<&str>) -> Id {
        let mut st = self.lock();
        let id = next_id(&mut st.next_id);
        if let Some(g) = st.guilds.get_mut(&guild) {
            let info = WebhookInfo {
                id,
                kind,
                name: name.to_owned(),
                channel_id: channel,
                creator_id: Some(Id(1)),
                creator_name: Some("owner".to_owned()),
                source: (kind == WebhookKind::ChannelFollower).then(|| "news".to_owned()),
                avatar: None,
            };
            g.res
                .get::<WebhooksState>()
                .hooks
                .insert(id, FakeWebhook { info, token: token.map(str::to_owned), avatar: None });
        }
        id
    }

    fn serve_avatar(&self, w: &WebhookInfo, image: Option<(String, Vec<u8>)>) {
        if let Some((h, bytes)) = image {
            self.set_url(&guildsmith_core::live::avatar_url(w.id, &h), bytes);
        }
    }

    /// Makes `GET webhooks/{id}` return tokens (or not).
    pub fn set_webhook_tokens_returned(&self, guild: Id, returned: bool) {
        self.edit_res::<WebhooksState>(guild, |s| s.return_tokens = returned);
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl WebhooksPort for FakeDiscord {
    async fn channel_webhooks(&self, channel: Id) -> PortResult<Vec<WebhookInfo>> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("channel_webhooks", guild, None, |g| {
            g.bot_can(Permission::ManageWebhooks)?;
            Ok(g.res
                .peek::<WebhooksState>()
                .map(|s| s.hooks.values().filter(|h| h.info.channel_id == channel).map(|h| h.info.clone()).collect())
                .unwrap_or_default())
        })
    }

    async fn guild_webhooks(&self, guild: Id) -> PortResult<Vec<WebhookInfo>> {
        self.read_guild("guild_webhooks", guild, None, |g| {
            g.bot_can(Permission::ManageWebhooks)?;
            Ok(g.res
                .peek::<WebhooksState>()
                .map(|s| s.hooks.values().map(|h| h.info.clone()).collect())
                .unwrap_or_default())
        })
    }

    async fn create_webhook(
        &self,
        channel: Id,
        name: &str,
        avatar: Option<&str>,
        reason: &str,
    ) -> PortResult<(WebhookInfo, Secret)> {
        let guild = self.guild_of_channel(channel)?;
        let image = avatar.and_then(uploaded);
        let created = self.write("create_webhook", guild, Some(channel), Some(reason), |g, next| {
            g.bot_can(Permission::ManageWebhooks)?;
            check_name(name)?;
            let bot = g.snapshot.bot.user_id;
            let s = g.res.get::<WebhooksState>();
            if s.hooks.values().filter(|h| h.info.channel_id == channel).count() >= 15 {
                return Err(bad(30007, "Maximum number of webhooks reached (15)"));
            }
            if s.hooks.len() >= 1000 {
                return Err(bad(30058, "Maximum number of webhooks per guild reached (1000)"));
            }
            let id = next_id(next);
            let token = format!("fake-webhook-token-{id}-0123456789abcdef");
            let info = WebhookInfo {
                id,
                kind: WebhookKind::Incoming,
                name: name.to_owned(),
                channel_id: channel,
                creator_id: Some(bot),
                creator_name: Some("guildsmith".to_owned()),
                source: None,
                avatar: image.as_ref().map(|(h, _)| h.clone()),
            };
            s.hooks.insert(
                id,
                FakeWebhook { info: info.clone(), token: Some(token.clone()), avatar: avatar.map(str::to_owned) },
            );
            Ok((info, Secret::new(token)))
        })?;
        self.serve_avatar(&created.0, image);
        Ok(created)
    }

    async fn update_webhook(&self, webhook: Id, f: &WebhookFields, reason: &str) -> PortResult<WebhookInfo> {
        let guild = self.guild_of_webhook(webhook)?;
        let image = f.avatar.clone().flatten().as_deref().and_then(uploaded);
        let updated = self.write("update_webhook", guild, Some(webhook), Some(reason), |g, _| {
            g.bot_can(Permission::ManageWebhooks)?;
            if let Some(n) = &f.name {
                check_name(n)?;
            }
            if let Some(c) = f.channel_id
                && g.snapshot.channel(c).is_none()
            {
                return Err(not_found("channel", c));
            }
            let h =
                g.res.get::<WebhooksState>().hooks.get_mut(&webhook).ok_or_else(|| not_found("webhook", webhook))?;
            if let Some(n) = &f.name {
                h.info.name.clone_from(n);
            }
            if let Some(a) = &f.avatar {
                h.avatar.clone_from(a);
                h.info.avatar = image.as_ref().map(|(hash, _)| hash.clone());
            }
            if let Some(c) = f.channel_id {
                h.info.channel_id = c;
            }
            Ok(h.info.clone())
        })?;
        self.serve_avatar(&updated, image);
        Ok(updated)
    }

    async fn delete_webhook(&self, webhook: Id, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_webhook(webhook)?;
        self.write("delete_webhook", guild, Some(webhook), Some(reason), |g, _| {
            g.bot_can(Permission::ManageWebhooks)?;
            g.res.get::<WebhooksState>().hooks.remove(&webhook).map(|_| ()).ok_or_else(|| not_found("webhook", webhook))
        })
    }

    async fn webhook_token(&self, webhook: Id) -> PortResult<Option<Secret>> {
        let guild = self.guild_of_webhook(webhook)?;
        self.read_guild("webhook_token", guild, Some(webhook), |g| {
            let s = g.res.peek::<WebhooksState>();
            let returned = s.is_some_and(|s| s.return_tokens);
            Ok(s.and_then(|s| s.hooks.get(&webhook))
                .and_then(|h| h.token.clone())
                .filter(|_| returned)
                .map(Secret::new))
        })
    }

    async fn execute_webhook(
        &self,
        webhook: Id,
        token: &Secret,
        msg: &OutgoingMessage,
        thread: Option<Id>,
        username: Option<&str>,
        _avatar_url: Option<&str>,
    ) -> PortResult<MessageInfo> {
        let guild = self.guild_of_webhook(webhook)?;
        self.write("execute_webhook", guild, Some(webhook), None, |g, next| {
            let h = g
                .res
                .peek::<WebhooksState>()
                .and_then(|s| s.hooks.get(&webhook))
                .cloned()
                .ok_or_else(|| not_found("webhook", webhook))?;
            if h.token.as_deref() != Some(token.expose()) {
                return Err(PortError::Unauthorized);
            }
            let channel = thread.unwrap_or(h.info.channel_id);
            let id = g.add_bot_message(channel, msg, next)?;
            if let Some(m) = g.messages.get_mut(&channel).and_then(|l| l.iter_mut().find(|m| m.id == id)) {
                m.author_id = webhook;
                username.unwrap_or(&h.info.name).clone_into(&mut m.author_name);
            }
            g.info(channel, id)
        })
    }
}
