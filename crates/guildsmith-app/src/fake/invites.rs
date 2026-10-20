//! `InvitesPort` for the fake: `max_age` ≤ 604800, `max_uses` ≤ 100, invite roles below the
//! bot (with Manage Roles), at most 1000 invites per server, and target users.

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, next_id, not_found};
use crate::ports::{InviteInfo, InvitesPort, NewInvite, PortError, PortResult};

/// The fake's invites for one server.
#[derive(Debug, Clone, Default)]
pub struct InvitesState {
    pub invites: BTreeMap<String, InviteInfo>,
    pub targets: BTreeMap<String, BTreeSet<Id>>,
}

fn bad(message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(50035), message: message.to_owned() }
}

impl FakeDiscord {
    fn guild_of_invite(&self, code: &str) -> PortResult<Id> {
        self.lock()
            .guilds
            .values()
            .find(|g| g.res.peek::<InvitesState>().is_some_and(|s| s.invites.contains_key(code)))
            .map(|g| g.snapshot.guild.id)
            .ok_or_else(|| PortError::NotFound(format!("invite {code}")))
    }

    /// Adds uses to an invite, as if people joined with it.
    pub fn use_invite(&self, guild: Id, code: &str, uses: u32) {
        self.edit_res::<InvitesState>(guild, |s| {
            if let Some(i) = s.invites.get_mut(code) {
                i.uses += uses;
            }
        });
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl InvitesPort for FakeDiscord {
    async fn create_invite(&self, channel: Id, inv: &NewInvite, reason: &str) -> PortResult<InviteInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("create_invite", guild, Some(channel), Some(reason), |g, next| {
            g.bot_can(Permission::CreateInstantInvite)?;
            if inv.max_age > 604_800 {
                return Err(bad("max_age is at most 604800"));
            }
            if inv.max_uses > 100 {
                return Err(bad("max_uses is at most 100"));
            }
            if !inv.roles.is_empty() {
                g.bot_can(Permission::ManageRoles)?;
                for r in &inv.roles {
                    g.role_below_bot(*r)?;
                }
            }
            let bot = g.snapshot.bot.user_id;
            let s = g.res.get::<InvitesState>();
            if s.invites.len() >= 1000 {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(30016),
                    message: "Maximum number of invites reached (1000)".to_owned(),
                });
            }
            if !inv.unique
                && let Some(same) = s.invites.values().find(|i| {
                    i.channel_id == channel
                        && i.inviter_id == Some(bot)
                        && i.max_age == inv.max_age
                        && i.max_uses == inv.max_uses
                        && i.temporary == inv.temporary
                        && i.roles == inv.roles
                })
            {
                return Ok(same.clone());
            }
            let code = format!("inv{}", next_id(next));
            let info = InviteInfo {
                code: code.clone(),
                channel_id: channel,
                inviter_id: Some(bot),
                inviter: Some("guildsmith".to_owned()),
                max_age: inv.max_age,
                max_uses: inv.max_uses,
                uses: 0,
                temporary: inv.temporary,
                created_at: "2026-10-19T12:00:00Z".to_owned(),
                expires_at: (inv.max_age > 0)
                    .then(|| crate::date::rfc3339(super::FAKE_NOW.unsigned_abs() + u64::from(inv.max_age))),
                roles: inv.roles.clone(),
            };
            s.invites.insert(code, info.clone());
            Ok(info)
        })
    }

    async fn guild_invites(&self, guild: Id) -> PortResult<Vec<InviteInfo>> {
        self.read_guild("guild_invites", guild, None, |g| {
            g.bot_can(Permission::ManageGuild)?;
            Ok(g.res.peek::<InvitesState>().map(|s| s.invites.values().cloned().collect()).unwrap_or_default())
        })
    }

    async fn delete_invite(&self, code: &str, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_invite(code)?;
        self.write("delete_invite", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageChannels)?;
            g.res.get::<InvitesState>().invites.remove(code).map(|_| ()).ok_or_else(|| not_found("invite", Id(0)))
        })
    }

    async fn invite_target_users(&self, code: &str) -> PortResult<Vec<Id>> {
        let guild = self.guild_of_invite(code)?;
        self.read_guild("invite_target_users", guild, None, |g| {
            Ok(g.res
                .peek::<InvitesState>()
                .and_then(|s| s.targets.get(code))
                .map(|t| t.iter().copied().collect())
                .unwrap_or_default())
        })
    }

    async fn add_invite_target_users(&self, code: &str, users: &[Id]) -> PortResult<()> {
        if users.len() > 1000 {
            return Err(bad("at most 1000 users per request"));
        }
        let guild = self.guild_of_invite(code)?;
        self.write("add_invite_target_users", guild, None, None, |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            g.res.get::<InvitesState>().targets.entry(code.to_owned()).or_default().extend(users.iter().copied());
            Ok(())
        })
    }

    async fn remove_invite_target_users(&self, code: &str, users: &[Id]) -> PortResult<()> {
        if users.len() > 1000 {
            return Err(bad("at most 1000 users per request"));
        }
        let guild = self.guild_of_invite(code)?;
        self.write("remove_invite_target_users", guild, None, None, |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            if let Some(t) = g.res.get::<InvitesState>().targets.get_mut(code) {
                t.retain(|u| !users.contains(u));
            }
            Ok(())
        })
    }
}
