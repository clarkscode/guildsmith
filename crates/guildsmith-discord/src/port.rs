//! `DiscordPort` for the real Discord API, through twilight.

use guildsmith_app::ports::{
    ChannelFields, ChannelPosition, CurrentUser, DiscordPort, GuildSummary, Member, Message, PortError, PortResult,
    RoleFields, RolePosition,
};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;

use twilight_http::request::AuditLogReason;
use twilight_model::util::Timestamp;

use crate::client::LazyClient;
use crate::error::map_err;
use crate::{ids, retry, snapshot, write};

/// The production `DiscordPort`. Builds its HTTP client on first use.
pub struct TwilightDiscord {
    client: LazyClient,
}

impl TwilightDiscord {
    /// A client for the real Discord API. `token` may be `None`: the error then surfaces on
    /// first use, so the MCP server can still start and list its tools.
    #[must_use]
    pub fn new(token: Option<Secret>) -> Self {
        TwilightDiscord { client: LazyClient::new(token, None) }
    }

    /// Sends every request to `host:port` over plain HTTP (twilight's http-proxy, or a mock
    /// server in tests). Rate limiting is left to the proxy.
    #[must_use]
    pub fn with_proxy(token: Option<Secret>, host: String) -> Self {
        TwilightDiscord { client: LazyClient::new(token, Some(host)) }
    }
}

fn member(m: &twilight_model::guild::Member) -> Member {
    Member {
        id: ids::from_user(m.user.id),
        username: m.user.name.clone(),
        display_name: m.user.global_name.clone(),
        nick: m.nick.clone(),
        role_ids: m.roles.iter().map(|r| ids::from(*r)).collect(),
        timed_out: m.communication_disabled_until.is_some(),
    }
}

fn message(m: &twilight_model::channel::Message) -> Message {
    Message {
        id: ids::from(m.id),
        channel_id: ids::from(m.channel_id),
        author_id: ids::from_user(m.author.id),
        author_name: m.author.name.clone(),
        content: m.content.clone(),
        timestamp: m.timestamp.iso_8601().to_string(),
    }
}

impl DiscordPort for TwilightDiscord {
    async fn current_user(&self) -> PortResult<CurrentUser> {
        Ok(self.client.get().await?.user.clone())
    }

    async fn list_guilds(&self) -> PortResult<Vec<GuildSummary>> {
        let http = &self.client.get().await?.http;
        let guilds = retry::read(|| async {
            http.current_user_guilds().await.map_err(map_err)?.models().await.map_err(map_err)
        })
        .await?;
        Ok(guilds.into_iter().map(|g| GuildSummary { id: ids::from(g.id), name: g.name }).collect())
    }

    async fn snapshot(&self, guild: Id) -> PortResult<Snapshot> {
        snapshot::build(self.client.get().await?, guild).await
    }

    async fn member(&self, guild: Id, user: Id) -> PortResult<Member> {
        let http = &self.client.get().await?.http;
        let (g, u) = (ids::guild(guild)?, ids::user(user)?);
        let m =
            retry::read(|| async { http.guild_member(g, u).await.map_err(map_err)?.model().await.map_err(map_err) })
                .await?;
        Ok(member(&m))
    }

    async fn search_members(&self, guild: Id, query: &str, limit: u16) -> PortResult<Vec<Member>> {
        let http = &self.client.get().await?.http;
        let g = ids::guild(guild)?;
        let found = retry::read(|| async {
            http.search_guild_members(g, query).limit(limit).await.map_err(map_err)?.models().await.map_err(map_err)
        })
        .await?;
        Ok(found.iter().map(member).collect())
    }

    async fn create_role(&self, guild: Id, fields: &RoleFields, reason: &str) -> PortResult<Id> {
        write::create_role(&self.client.get().await?.http, guild, fields, reason).await
    }

    async fn update_role(&self, guild: Id, role: Id, fields: &RoleFields, reason: &str) -> PortResult<()> {
        write::update_role(&self.client.get().await?.http, guild, role, fields, reason).await
    }

    async fn delete_role(&self, guild: Id, role: Id, reason: &str) -> PortResult<()> {
        write::delete_role(&self.client.get().await?.http, guild, role, reason).await
    }

    async fn reorder_roles(&self, guild: Id, order: &[RolePosition], reason: &str) -> PortResult<()> {
        write::reorder_roles(&self.client.get().await?.http, guild, order, reason).await
    }

    async fn create_channel(&self, guild: Id, fields: &ChannelFields, reason: &str) -> PortResult<Id> {
        write::create_channel(&self.client.get().await?.http, guild, fields, reason).await
    }

    async fn update_channel(&self, _guild: Id, channel: Id, fields: &ChannelFields, reason: &str) -> PortResult<()> {
        write::update_channel(&self.client.get().await?.http, channel, fields, reason).await
    }

    async fn delete_channel(&self, _guild: Id, channel: Id, reason: &str) -> PortResult<()> {
        write::delete_channel(&self.client.get().await?.http, channel, reason).await
    }

    async fn reorder_channels(&self, guild: Id, positions: &[ChannelPosition], reason: &str) -> PortResult<()> {
        write::reorder_channels(&self.client.get().await?.http, guild, positions, reason).await
    }

    async fn add_member_role(&self, guild: Id, user: Id, role: Id, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        let (g, u, r) = (ids::guild(guild)?, ids::user(user)?, ids::role(role)?);
        http.add_guild_member_role(g, u, r).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn remove_member_role(&self, guild: Id, user: Id, role: Id, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        let (g, u, r) = (ids::guild(guild)?, ids::user(user)?, ids::role(role)?);
        http.remove_guild_member_role(g, u, r).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn send_message(&self, channel: Id, content: &str) -> PortResult<Message> {
        let http = &self.client.get().await?.http;
        let m = http
            .create_message(ids::channel(channel)?)
            .content(content)
            .await
            .map_err(map_err)?
            .model()
            .await
            .map_err(map_err)?;
        Ok(message(&m))
    }

    async fn read_messages(&self, channel: Id, limit: u8, before: Option<Id>) -> PortResult<Vec<Message>> {
        let http = &self.client.get().await?.http;
        let c = ids::channel(channel)?;
        let before = before.map(ids::message).transpose()?;
        let msgs = retry::read(|| async {
            let req = http.channel_messages(c);
            let res = match before {
                Some(b) => req.before(b).limit(u16::from(limit)).await,
                None => req.limit(u16::from(limit)).await,
            };
            res.map_err(map_err)?.models().await.map_err(map_err)
        })
        .await?;
        Ok(msgs.iter().map(message).collect())
    }

    async fn ban(&self, guild: Id, user: Id, delete_message_seconds: u32, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        let mut req = http.create_ban(ids::guild(guild)?, ids::user(user)?);
        if delete_message_seconds > 0 {
            req = req.delete_message_seconds(delete_message_seconds);
        }
        req.reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn unban(&self, guild: Id, user: Id, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        http.delete_ban(ids::guild(guild)?, ids::user(user)?).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn kick(&self, guild: Id, user: Id, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        http.remove_guild_member(ids::guild(guild)?, ids::user(user)?).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn timeout(&self, guild: Id, user: Id, until: Option<&str>, reason: &str) -> PortResult<()> {
        let http = &self.client.get().await?.http;
        let until = until
            .map(|t| {
                Timestamp::parse(t).map_err(|e| PortError::Discord {
                    status: 400,
                    code: None,
                    message: format!("bad timeout timestamp `{t}`: {e}"),
                })
            })
            .transpose()?;
        http.update_guild_member(ids::guild(guild)?, ids::user(user)?)
            .communication_disabled_until(until)
            .reason(reason)
            .await
            .map_err(map_err)?;
        Ok(())
    }
}
