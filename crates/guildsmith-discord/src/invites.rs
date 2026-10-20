//! `InvitesPort` for the real Discord API (`ext::invites` for invite roles and target users).

use guildsmith_app::ports::{InviteInfo, InvitesPort, NewInvite, PortResult};
use guildsmith_core::Id;
use twilight_http::request::Method;

use crate::ext::invites::{self, RawInvite};
use crate::port::TwilightDiscord;
use crate::raw;

impl InvitesPort for TwilightDiscord {
    async fn create_invite(&self, channel: Id, invite: &NewInvite, reason: &str) -> PortResult<InviteInfo> {
        invites::create(self.http().await?, channel, invite, reason).await
    }

    async fn guild_invites(&self, guild: Id) -> PortResult<Vec<InviteInfo>> {
        let http = self.http().await?;
        let list: Vec<RawInvite> =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/invites"), &[], None, None).await?;
        Ok(list.into_iter().map(InviteInfo::from).collect())
    }

    async fn delete_invite(&self, code: &str, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("invites/{code}"), &[], None, Some(reason)).await?;
        Ok(())
    }

    async fn invite_target_users(&self, code: &str) -> PortResult<Vec<Id>> {
        invites::target_users(self.http().await?, code).await
    }

    async fn add_invite_target_users(&self, code: &str, users: &[Id]) -> PortResult<()> {
        invites::set_targets(self.http().await?, code, users, true).await
    }

    async fn remove_invite_target_users(&self, code: &str, users: &[Id]) -> PortResult<()> {
        invites::set_targets(self.http().await?, code, users, false).await
    }
}
