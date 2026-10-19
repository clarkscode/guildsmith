//! Builds a `Snapshot`: one guild GET (with roles), the channel list, and the bot's member record.

use guildsmith_app::ports::{PortError, PortResult};
use guildsmith_core::snapshot::{Completeness, Snapshot};
use guildsmith_core::{Id, Permission};

use crate::client::Ready;
use crate::error::map_err;
use crate::{ids, map, retry};

pub(crate) async fn build(ready: &Ready, guild: Id) -> PortResult<Snapshot> {
    let http = &ready.http;
    let gid = ids::guild(guild)?;

    let g = retry::read(|| async { http.guild(gid).await.map_err(map_err)?.model().await.map_err(map_err) })
        .await
        .map_err(|e| match e {
            PortError::NotFound(_) | PortError::Forbidden(_) => PortError::NotInGuild(guild),
            other => other,
        })?;
    let channels =
        retry::read(|| async { http.guild_channels(gid).await.map_err(map_err)?.models().await.map_err(map_err) })
            .await?;
    let bot_id = ids::user(ready.user.id)?;
    let member =
        retry::read(|| async { http.guild_member(gid, bot_id).await.map_err(map_err)?.model().await.map_err(map_err) })
            .await?;

    let live_guild = map::guild::live(&g);
    let roles: Vec<_> = g.roles.iter().map(map::role::live).collect();
    let bot =
        map::guild::bot_info(&live_guild, &roles, ready.user.id, member.roles.iter().map(|r| ids::from(*r)).collect());
    let completeness = if bot.permissions.contains(Permission::Administrator) {
        Completeness::Complete
    } else {
        Completeness::MaybeIncomplete
    };
    Ok(Snapshot {
        guild: live_guild,
        roles,
        channels: channels.iter().map(map::channel::live).collect(),
        bot,
        completeness,
    })
}
