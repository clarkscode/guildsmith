//! `DiscordPort` for the real Discord API, through twilight.

use guildsmith_app::ports::{
    ChannelFields, ChannelPosition, CurrentUser, DiscordPort, GuildSummary, Member, Message, PortError, PortResult,
    RoleFields, RolePosition,
};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;

use crate::client::LazyClient;
use crate::error::map_err;
use crate::{ids, retry, snapshot};

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

fn not_yet<T>(what: &str) -> PortResult<T> {
    Err(PortError::Unsupported(format!("{what} is not implemented in this build yet")))
}

// Write methods are stubs until US2/US4 and don't await yet; remove this allow then.
#[allow(clippy::unused_async_trait_impl)]
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

    async fn create_role(&self, _guild: Id, _fields: &RoleFields, _reason: &str) -> PortResult<Id> {
        not_yet("create_role")
    }

    async fn update_role(&self, _guild: Id, _role: Id, _fields: &RoleFields, _reason: &str) -> PortResult<()> {
        not_yet("update_role")
    }

    async fn delete_role(&self, _guild: Id, _role: Id, _reason: &str) -> PortResult<()> {
        not_yet("delete_role")
    }

    async fn reorder_roles(&self, _guild: Id, _order: &[RolePosition], _reason: &str) -> PortResult<()> {
        not_yet("reorder_roles")
    }

    async fn create_channel(&self, _guild: Id, _fields: &ChannelFields, _reason: &str) -> PortResult<Id> {
        not_yet("create_channel")
    }

    async fn update_channel(&self, _guild: Id, _channel: Id, _fields: &ChannelFields, _reason: &str) -> PortResult<()> {
        not_yet("update_channel")
    }

    async fn delete_channel(&self, _guild: Id, _channel: Id, _reason: &str) -> PortResult<()> {
        not_yet("delete_channel")
    }

    async fn reorder_channels(&self, _guild: Id, _positions: &[ChannelPosition], _reason: &str) -> PortResult<()> {
        not_yet("reorder_channels")
    }

    async fn add_member_role(&self, _guild: Id, _user: Id, _role: Id, _reason: &str) -> PortResult<()> {
        not_yet("add_member_role")
    }

    async fn remove_member_role(&self, _guild: Id, _user: Id, _role: Id, _reason: &str) -> PortResult<()> {
        not_yet("remove_member_role")
    }

    async fn send_message(&self, _channel: Id, _content: &str) -> PortResult<Message> {
        not_yet("send_message")
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

    async fn ban(&self, _guild: Id, _user: Id, _delete_message_seconds: u32, _reason: &str) -> PortResult<()> {
        not_yet("ban")
    }

    async fn unban(&self, _guild: Id, _user: Id, _reason: &str) -> PortResult<()> {
        not_yet("unban")
    }

    async fn kick(&self, _guild: Id, _user: Id, _reason: &str) -> PortResult<()> {
        not_yet("kick")
    }

    async fn timeout(&self, _guild: Id, _user: Id, _until: Option<&str>, _reason: &str) -> PortResult<()> {
        not_yet("timeout")
    }
}
