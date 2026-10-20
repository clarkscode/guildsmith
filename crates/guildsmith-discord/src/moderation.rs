//! `ModerationPort` for the real Discord API: twilight builders for the ban list and message
//! deletion; `ext` for bulk ban, prune, the audit log and incident actions.

use guildsmith_app::ports::{AuditEntry, BanEntry, BulkBanResult, Incidents, ModerationPort, PortResult};
use guildsmith_core::Id;
use twilight_http::request::AuditLogReason;

use crate::error::map_err;
use crate::port::TwilightDiscord;
use crate::{ext, ids, retry};

impl ModerationPort for TwilightDiscord {
    async fn bans(&self, guild: Id, limit: u16, before: Option<Id>, after: Option<Id>) -> PortResult<Vec<BanEntry>> {
        let http = self.http().await?;
        let g = ids::guild(guild)?;
        let (before, after) = (before.map(ids::user).transpose()?, after.map(ids::user).transpose()?);
        let bans = retry::read(|| async {
            let mut req = http.bans(g).limit(limit);
            if let Some(b) = before {
                req = req.before(b);
            }
            if let Some(a) = after {
                req = req.after(a);
            }
            req.await.map_err(map_err)?.models().await.map_err(map_err)
        })
        .await?;
        Ok(bans
            .into_iter()
            .map(|b| BanEntry { user_id: ids::from_user(b.user.id), username: b.user.name, reason: b.reason })
            .collect())
    }

    async fn bulk_ban(&self, guild: Id, users: &[Id], delete: u32, reason: &str) -> PortResult<BulkBanResult> {
        ext::bans::bulk_ban(self.http().await?, guild, users, delete, reason).await
    }

    async fn prune_count(&self, guild: Id, days: u8, roles: &[Id]) -> PortResult<u64> {
        let http = self.http().await?;
        retry::read(|| ext::moderation::prune_count(http, guild, days, roles)).await
    }

    async fn prune(&self, guild: Id, days: u8, roles: &[Id], compute: bool, reason: &str) -> PortResult<Option<u64>> {
        ext::moderation::prune(self.http().await?, guild, days, roles, compute, reason).await
    }

    async fn bulk_delete(&self, channel: Id, messages: &[Id], reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        let ids = messages.iter().map(|m| ids::message(*m)).collect::<PortResult<Vec<_>>>()?;
        http.delete_messages(ids::channel(channel)?, &ids).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn delete_message(&self, channel: Id, message: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        http.delete_message(ids::channel(channel)?, ids::message(message)?).reason(reason).await.map_err(map_err)?;
        Ok(())
    }

    async fn audit_log(
        &self,
        guild: Id,
        action_type: Option<u16>,
        user: Option<Id>,
        before: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<AuditEntry>> {
        ext::moderation::audit_log(self.http().await?, guild, action_type, user, before, limit).await
    }

    async fn set_incidents(&self, guild: Id, incidents: &Incidents, reason: &str) -> PortResult<Incidents> {
        ext::moderation::set_incidents(self.http().await?, guild, incidents, reason).await
    }
}
