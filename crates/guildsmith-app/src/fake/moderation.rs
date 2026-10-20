//! `ModerationPort` for the fake: bans with reasons, prune candidates, the 14-day bulk-delete
//! rule, the audit log and incident actions.

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, FakeGuild, not_found};
use crate::date;
use crate::ports::{AuditEntry, BanEntry, BulkBanResult, Incidents, ModerationPort, PortError, PortResult};

/// Bulk delete refuses messages older than this.
pub const BULK_DELETE_MAX_AGE: i64 = 14 * 86_400;

/// The fake's moderation state for one server.
#[derive(Debug, Clone, Default)]
pub struct ModerationState {
    /// Ban reasons, by user (v1's `bans` list holds who is banned).
    pub ban_reasons: BTreeMap<Id, Option<String>>,
    /// Members a prune would remove (if their roles allow it).
    pub inactive: BTreeSet<Id>,
    /// Audit log entries, oldest first.
    pub audit: Vec<AuditEntry>,
    pub incidents: Incidents,
}

impl FakeDiscord {
    /// Marks members as inactive, so a prune would remove them.
    pub fn set_inactive(&self, guild: Id, members: &[Id]) {
        self.edit_res::<ModerationState>(guild, |m| m.inactive.extend(members.iter().copied()));
    }

    /// Adds a message with a given RFC 3339 timestamp, as if a user posted it then.
    pub fn post_at(&self, guild: Id, channel: Id, author: &crate::ports::Member, content: &str, timestamp: &str) -> Id {
        let mut st = self.lock();
        let id = super::next_id(&mut st.next_id);
        if let Some(g) = st.guilds.get_mut(&guild) {
            g.messages.entry(channel).or_default().push(crate::ports::Message {
                id,
                channel_id: channel,
                author_id: author.id,
                author_name: author.username.clone(),
                content: content.to_owned(),
                timestamp: timestamp.to_owned(),
            });
        }
        id
    }
}

fn role_ok(g: &FakeGuild, user: Id) -> PortResult<()> {
    let s = &g.snapshot;
    if user == s.guild.owner_id {
        return Err(PortError::Forbidden("the server owner can't be banned".to_owned()));
    }
    if s.bot.user_id == s.guild.owner_id {
        return Ok(());
    }
    let top = g
        .members
        .iter()
        .find(|m| m.id == user)
        .map_or(0, |m| m.role_ids.iter().filter_map(|r| s.role(*r)).map(|r| r.position).max().unwrap_or(0));
    if top >= s.bot.top_role_position {
        return Err(PortError::Forbidden("the member's highest role is at or above the bot's".to_owned()));
    }
    Ok(())
}

fn prunable(g: &FakeGuild, roles: &[Id]) -> Vec<Id> {
    let inactive = g.res.peek::<ModerationState>().map(|m| m.inactive.clone()).unwrap_or_default();
    g.members
        .iter()
        .filter(|m| inactive.contains(&m.id) && m.role_ids.iter().all(|r| roles.contains(r)))
        .map(|m| m.id)
        .collect()
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl ModerationPort for FakeDiscord {
    async fn bans(&self, guild: Id, limit: u16, before: Option<Id>, after: Option<Id>) -> PortResult<Vec<BanEntry>> {
        self.read_guild("bans", guild, None, |g| {
            g.bot_can(Permission::BanMembers)?;
            let reasons = g.res.peek::<ModerationState>().map(|m| m.ban_reasons.clone()).unwrap_or_default();
            let mut ids = g.bans.clone();
            ids.sort();
            Ok(ids
                .into_iter()
                .filter(|id| before.is_none_or(|b| *id < b) && after.is_none_or(|a| *id > a))
                .take(usize::from(limit))
                .map(|id| BanEntry {
                    user_id: id,
                    username: format!("user{id}"),
                    reason: reasons.get(&id).cloned().flatten(),
                })
                .collect())
        })
    }

    async fn bulk_ban(&self, guild: Id, users: &[Id], _delete: u32, reason: &str) -> PortResult<BulkBanResult> {
        self.write("bulk_ban", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::BanMembers)?;
            g.bot_can(Permission::ManageGuild)?;
            if users.len() > 200 {
                return Err(PortError::Discord { status: 400, code: None, message: "at most 200 users".to_owned() });
            }
            let mut out = BulkBanResult::default();
            for u in users {
                if role_ok(g, *u).is_ok() && !g.bans.contains(u) {
                    g.members.retain(|m| m.id != *u);
                    g.bans.push(*u);
                    g.res.get::<ModerationState>().ban_reasons.insert(*u, Some(reason.to_owned()));
                    out.banned.push(*u);
                } else {
                    out.failed.push(*u);
                }
            }
            if out.banned.is_empty() {
                return Err(PortError::Discord {
                    status: 500,
                    code: Some(500_000),
                    message: "Failed to ban users".to_owned(),
                });
            }
            Ok(out)
        })
    }

    async fn prune_count(&self, guild: Id, days: u8, roles: &[Id]) -> PortResult<u64> {
        if !(1..=30).contains(&days) {
            return Err(PortError::Discord { status: 400, code: None, message: "days must be 1–30".to_owned() });
        }
        self.read_guild("prune_count", guild, None, |g| {
            g.bot_can(Permission::KickMembers)?;
            Ok(prunable(g, roles).len() as u64)
        })
    }

    async fn prune(&self, guild: Id, days: u8, roles: &[Id], compute: bool, reason: &str) -> PortResult<Option<u64>> {
        if !(1..=30).contains(&days) {
            return Err(PortError::Discord { status: 400, code: None, message: "days must be 1–30".to_owned() });
        }
        self.write("prune", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::KickMembers)?;
            let gone = prunable(g, roles);
            g.members.retain(|m| !gone.contains(&m.id));
            Ok(compute.then_some(gone.len() as u64))
        })
    }

    async fn bulk_delete(&self, channel: Id, messages: &[Id], reason: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        let now = self.now_secs();
        self.write("bulk_delete", guild, Some(channel), Some(reason), |g, _| {
            g.bot_can(Permission::ManageMessages)?;
            if !(2..=100).contains(&messages.len()) {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(50016),
                    message: "2–100 messages".to_owned(),
                });
            }
            let list = g.messages.entry(channel).or_default();
            for id in messages {
                let m = list.iter().find(|m| m.id == *id).ok_or_else(|| not_found("message", *id))?;
                let at = date::parse_rfc3339(&m.timestamp).unwrap_or(0);
                if now - at > BULK_DELETE_MAX_AGE {
                    // Discord fails the whole request.
                    return Err(PortError::Discord {
                        status: 400,
                        code: Some(50034),
                        message: "You can only bulk delete messages that are under 14 days old.".to_owned(),
                    });
                }
            }
            list.retain(|m| !messages.contains(&m.id));
            Ok(())
        })
    }

    async fn delete_message(&self, channel: Id, message: Id, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("delete_message", guild, Some(message), Some(reason), |g, _| {
            let author = g
                .messages
                .get(&channel)
                .and_then(|l| l.iter().find(|m| m.id == message))
                .map(|m| m.author_id)
                .ok_or_else(|| not_found("message", message))?;
            if author != g.snapshot.bot.user_id {
                g.bot_can(Permission::ManageMessages)?;
            }
            g.messages.entry(channel).or_default().retain(|m| m.id != message);
            Ok(())
        })
    }

    async fn audit_log(
        &self,
        guild: Id,
        action_type: Option<u16>,
        user: Option<Id>,
        before: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<AuditEntry>> {
        self.read_guild("audit_log", guild, None, |g| {
            g.bot_can(Permission::ViewAuditLog)?;
            let all = g.res.peek::<ModerationState>().map(|m| m.audit.clone()).unwrap_or_default();
            Ok(all
                .into_iter()
                .rev()
                .filter(|e| action_type.is_none_or(|t| e.action_type == t))
                .filter(|e| user.is_none_or(|u| e.user_id == Some(u)))
                .filter(|e| before.is_none_or(|b| e.id < b))
                .take(usize::from(limit))
                .collect())
        })
    }

    async fn set_incidents(&self, guild: Id, incidents: &Incidents, reason: &str) -> PortResult<Incidents> {
        self.write("set_incidents", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            g.res.get::<ModerationState>().incidents = incidents.clone();
            Ok(incidents.clone())
        })
    }
}
