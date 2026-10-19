//! `FakeDiscord`: an in-memory `DiscordPort` for tests and demos (feature `fake`).
//!
//! It applies the Discord rules guildsmith relies on: the role hierarchy, `@everyone` (id equal
//! to the server id) can't be deleted or renamed, managed roles can't be deleted, and a bulk
//! channel reorder may change at most one `parent_id`. Every write is recorded with its reason,
//! and failures can be injected.

use std::collections::BTreeMap;
use std::sync::Mutex;

use guildsmith_core::snapshot::{Completeness, LiveChannel, LiveChannelKind, LiveRole, LiveTag, Snapshot};
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission, PermissionSet};

use crate::ports::{
    ChannelFields, ChannelPosition, CurrentUser, DiscordPort, GuildSummary, Member, Message, PortError, PortResult,
    RoleFields, RolePosition,
};

/// One recorded write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteRecord {
    pub method: &'static str,
    pub guild: Option<Id>,
    pub target: Option<Id>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
struct FakeGuild {
    snapshot: Snapshot,
    members: Vec<Member>,
    messages: BTreeMap<Id, Vec<Message>>,
    bans: Vec<Id>,
}

#[derive(Debug)]
struct State {
    user: CurrentUser,
    guilds: BTreeMap<Id, FakeGuild>,
    writes: Vec<WriteRecord>,
    reads: usize,
    /// Fail the n-th write (1-based, counting every write ever made).
    fail_write_at: Option<usize>,
    /// Fail the next call of this method name.
    fail_method: Option<&'static str>,
    next_id: u64,
}

/// An in-memory Discord.
#[derive(Debug)]
pub struct FakeDiscord {
    state: Mutex<State>,
}

fn not_found(what: &str, id: Id) -> PortError {
    PortError::NotFound(format!("{what} {id}"))
}

fn server_error() -> PortError {
    PortError::Discord { status: 500, code: None, message: "injected failure".to_owned() }
}

impl FakeGuild {
    fn recompute_bot(&mut self) {
        let s = &mut self.snapshot;
        let everyone = s.roles.iter().find(|r| r.id == s.guild.id).map_or(PermissionSet::EMPTY, |r| r.permissions);
        let mut perms = everyone;
        let mut top = 0;
        for id in &s.bot.role_ids {
            if let Some(r) = s.roles.iter().find(|r| r.id == *id) {
                perms = perms.union(r.permissions);
                top = top.max(r.position);
            }
        }
        let admin = perms.contains(Permission::Administrator) || s.bot.user_id == s.guild.owner_id;
        if admin {
            perms = PermissionSet::all_known();
        }
        s.bot.permissions = perms;
        s.bot.top_role_position = top;
        s.completeness = if admin { Completeness::Complete } else { Completeness::MaybeIncomplete };
    }

    fn bot_can(&self, p: Permission) -> PortResult<()> {
        if self.snapshot.bot.permissions.contains(p) { Ok(()) } else { Err(PortError::Forbidden(p.name().to_owned())) }
    }

    fn role_below_bot(&self, role: Id) -> PortResult<&LiveRole> {
        let r = self.snapshot.role(role).ok_or_else(|| not_found("role", role))?;
        if self.snapshot.bot.user_id != self.snapshot.guild.owner_id
            && r.position >= self.snapshot.bot.top_role_position
        {
            return Err(PortError::Forbidden(format!("role @{} is at or above the bot's highest role", r.name)));
        }
        Ok(r)
    }

    fn apply_channel_fields(ch: &mut LiveChannel, f: &ChannelFields, next_id: &mut u64) {
        if let Some(n) = &f.name {
            ch.name.clone_from(n);
        }
        if let Some(k) = f.kind {
            ch.kind = k;
        }
        if let Some(p) = f.parent_id {
            ch.parent_id = p;
        }
        if let Some(p) = f.position {
            ch.position = p;
        }
        if let Some(t) = &f.topic {
            ch.topic = if t.is_empty() { None } else { Some(t.clone()) };
        }
        if let Some(v) = f.nsfw {
            ch.nsfw = v;
        }
        if let Some(v) = f.slowmode {
            ch.slowmode = v;
        }
        if let Some(v) = f.bitrate {
            ch.bitrate = Some(v);
        }
        if let Some(v) = f.user_limit {
            ch.user_limit = Some(v);
        }
        if let Some(v) = &f.rtc_region {
            ch.rtc_region.clone_from(v);
        }
        if let Some(tags) = &f.tags {
            ch.tags = tags
                .iter()
                .map(|t| LiveTag {
                    id: t.id.unwrap_or_else(|| {
                        *next_id += 1;
                        Id(*next_id)
                    }),
                    name: t.name.clone(),
                    emoji: t.emoji.clone(),
                    moderated: t.moderated.unwrap_or(false),
                })
                .collect();
        }
        if let Some(v) = &f.default_reaction {
            ch.default_reaction.clone_from(v);
        }
        if let Some(o) = &f.overwrites {
            ch.overwrites.clone_from(o);
        }
    }
}

impl FakeDiscord {
    /// A fake with the given servers. The bot user is taken from the first snapshot.
    #[must_use]
    pub fn new(guilds: Vec<(Snapshot, Vec<Member>)>) -> Self {
        let bot_id = guilds.first().map_or(Id(2), |(s, _)| s.bot.user_id);
        let mut map = BTreeMap::new();
        for (snapshot, members) in guilds {
            let mut g = FakeGuild { snapshot, members, messages: BTreeMap::new(), bans: Vec::new() };
            g.recompute_bot();
            map.insert(g.snapshot.guild.id, g);
        }
        FakeDiscord {
            state: Mutex::new(State {
                user: CurrentUser { id: bot_id, username: "guildsmith".to_owned(), bot: true },
                guilds: map,
                writes: Vec::new(),
                reads: 0,
                fail_write_at: None,
                fail_method: None,
                next_id: 5_000_000,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Makes the token look like a user account instead of a bot (FR-022 tests).
    pub fn set_user_account(&self) {
        self.lock().user.bot = false;
    }

    /// Fails the n-th write from now on (1-based, counted over every write).
    pub fn fail_write_at(&self, n: usize) {
        let mut st = self.lock();
        st.fail_write_at = Some(st.writes.len() + n);
    }

    /// Fails the next call of `method` (e.g. `"snapshot"`, `"create_role"`).
    pub fn fail_next(&self, method: &'static str) {
        self.lock().fail_method = Some(method);
    }

    /// Every write so far, in order.
    #[must_use]
    pub fn writes(&self) -> Vec<WriteRecord> {
        self.lock().writes.clone()
    }

    /// How many reads have been served.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.lock().reads
    }

    /// Edits a server directly, as if someone changed it in Discord (not recorded as a write).
    pub fn edit_guild(&self, guild: Id, f: impl FnOnce(&mut Snapshot)) {
        let mut st = self.lock();
        if let Some(g) = st.guilds.get_mut(&guild) {
            f(&mut g.snapshot);
            g.recompute_bot();
        }
    }

    /// The current state of a server.
    #[must_use]
    pub fn current(&self, guild: Id) -> Option<Snapshot> {
        self.lock().guilds.get(&guild).map(|g| g.snapshot.clone())
    }

    /// Adds a message to a channel, as if a user posted it.
    pub fn post_as_user(&self, guild: Id, channel: Id, author: &Member, content: &str) {
        let mut st = self.lock();
        st.next_id += 1;
        let id = Id(st.next_id);
        if let Some(g) = st.guilds.get_mut(&guild) {
            g.messages.entry(channel).or_default().push(Message {
                id,
                channel_id: channel,
                author_id: author.id,
                author_name: author.username.clone(),
                content: content.to_owned(),
                timestamp: "2026-10-19T12:00:00Z".to_owned(),
            });
        }
    }

    fn check_fail(st: &mut State, method: &'static str) -> PortResult<()> {
        if st.fail_method == Some(method) {
            st.fail_method = None;
            return Err(server_error());
        }
        Ok(())
    }

    fn read<T>(&self, method: &'static str, f: impl FnOnce(&State) -> PortResult<T>) -> PortResult<T> {
        let mut st = self.lock();
        Self::check_fail(&mut st, method)?;
        st.reads += 1;
        f(&st)
    }

    fn write<T>(
        &self,
        method: &'static str,
        guild: Id,
        target: Option<Id>,
        reason: Option<&str>,
        f: impl FnOnce(&mut FakeGuild, &mut u64) -> PortResult<T>,
    ) -> PortResult<T> {
        let mut st = self.lock();
        Self::check_fail(&mut st, method)?;
        if st.fail_write_at == Some(st.writes.len() + 1) {
            st.fail_write_at = None;
            return Err(server_error());
        }
        let mut next_id = st.next_id;
        let g = st.guilds.get_mut(&guild).ok_or(PortError::NotInGuild(guild))?;
        let out = f(g, &mut next_id)?;
        g.recompute_bot();
        st.next_id = next_id;
        st.writes.push(WriteRecord { method, guild: Some(guild), target, reason: reason.map(str::to_owned) });
        Ok(out)
    }

    fn guild_of_channel(&self, channel: Id) -> PortResult<Id> {
        self.lock()
            .guilds
            .values()
            .find(|g| g.snapshot.channel(channel).is_some())
            .map(|g| g.snapshot.guild.id)
            .ok_or_else(|| not_found("channel", channel))
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl DiscordPort for FakeDiscord {
    async fn current_user(&self) -> PortResult<CurrentUser> {
        self.read("current_user", |st| Ok(st.user.clone()))
    }

    async fn list_guilds(&self) -> PortResult<Vec<GuildSummary>> {
        self.read("list_guilds", |st| {
            Ok(st
                .guilds
                .values()
                .map(|g| GuildSummary { id: g.snapshot.guild.id, name: g.snapshot.guild.name.clone() })
                .collect())
        })
    }

    async fn snapshot(&self, guild: Id) -> PortResult<Snapshot> {
        self.read("snapshot", |st| {
            st.guilds.get(&guild).map(|g| g.snapshot.clone()).ok_or(PortError::NotInGuild(guild))
        })
    }

    async fn member(&self, guild: Id, user: Id) -> PortResult<Member> {
        self.read("member", |st| {
            let g = st.guilds.get(&guild).ok_or(PortError::NotInGuild(guild))?;
            g.members.iter().find(|m| m.id == user).cloned().ok_or_else(|| not_found("member", user))
        })
    }

    async fn search_members(&self, guild: Id, query: &str, limit: u16) -> PortResult<Vec<Member>> {
        let q = query.to_lowercase();
        self.read("search_members", |st| {
            let g = st.guilds.get(&guild).ok_or(PortError::NotInGuild(guild))?;
            Ok(g.members
                .iter()
                .filter(|m| {
                    m.username.to_lowercase().starts_with(&q)
                        || m.nick.as_deref().is_some_and(|n| n.to_lowercase().starts_with(&q))
                })
                .take(usize::from(limit))
                .cloned()
                .collect())
        })
    }

    async fn create_role(&self, guild: Id, fields: &RoleFields, reason: &str) -> PortResult<Id> {
        self.write("create_role", guild, None, Some(reason), |g, next| {
            g.bot_can(Permission::ManageRoles)?;
            *next += 1;
            let id = Id(*next);
            // Discord puts new roles at position 1, above @everyone.
            for r in g.snapshot.roles.iter_mut().filter(|r| r.id != g.snapshot.guild.id) {
                r.position += 1;
            }
            g.snapshot.roles.push(LiveRole {
                id,
                name: fields.name.clone().unwrap_or_else(|| "new role".to_owned()),
                color: fields.color.unwrap_or(0),
                secondary_color: None,
                tertiary_color: None,
                hoist: fields.hoist.unwrap_or(false),
                mentionable: fields.mentionable.unwrap_or(false),
                managed: false,
                permissions: fields.permissions.unwrap_or_default(),
                position: 1,
                tags: None,
            });
            Ok(id)
        })
    }

    async fn update_role(&self, guild: Id, role: Id, fields: &RoleFields, reason: &str) -> PortResult<()> {
        self.write("update_role", guild, Some(role), Some(reason), |g, _| {
            g.bot_can(Permission::ManageRoles)?;
            let is_everyone = role == g.snapshot.guild.id;
            if !is_everyone {
                g.role_below_bot(role)?;
            }
            if is_everyone && fields.name.is_some() {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(50028),
                    message: "can't rename @everyone".to_owned(),
                });
            }
            let r = g.snapshot.roles.iter_mut().find(|r| r.id == role).ok_or_else(|| not_found("role", role))?;
            if let Some(n) = &fields.name {
                r.name.clone_from(n);
            }
            if let Some(c) = fields.color {
                r.color = c;
            }
            if let Some(h) = fields.hoist {
                r.hoist = h;
            }
            if let Some(m) = fields.mentionable {
                r.mentionable = m;
            }
            if let Some(p) = fields.permissions {
                r.permissions = p;
            }
            Ok(())
        })
    }

    async fn delete_role(&self, guild: Id, role: Id, reason: &str) -> PortResult<()> {
        self.write("delete_role", guild, Some(role), Some(reason), |g, _| {
            g.bot_can(Permission::ManageRoles)?;
            if role == g.snapshot.guild.id {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(50028),
                    message: "can't delete @everyone".to_owned(),
                });
            }
            if g.role_below_bot(role)?.managed {
                return Err(PortError::Forbidden("managed roles can't be deleted".to_owned()));
            }
            g.snapshot.roles.retain(|r| r.id != role);
            for m in &mut g.members {
                m.role_ids.retain(|r| *r != role);
            }
            Ok(())
        })
    }

    async fn reorder_roles(&self, guild: Id, order: &[RolePosition], reason: &str) -> PortResult<()> {
        self.write("reorder_roles", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageRoles)?;
            for p in order {
                if p.id == g.snapshot.guild.id {
                    return Err(PortError::Discord {
                        status: 400,
                        code: None,
                        message: "can't move @everyone".to_owned(),
                    });
                }
                g.role_below_bot(p.id)?;
                if p.position >= g.snapshot.bot.top_role_position && g.snapshot.bot.user_id != g.snapshot.guild.owner_id
                {
                    return Err(PortError::Forbidden(
                        "can't move a role to or above the bot's highest role".to_owned(),
                    ));
                }
            }
            for p in order {
                if let Some(r) = g.snapshot.roles.iter_mut().find(|r| r.id == p.id) {
                    r.position = p.position;
                }
            }
            Ok(())
        })
    }

    async fn create_channel(&self, guild: Id, fields: &ChannelFields, reason: &str) -> PortResult<Id> {
        self.write("create_channel", guild, None, Some(reason), |g, next| {
            g.bot_can(Permission::ManageChannels)?;
            if let Some(Some(parent)) = fields.parent_id {
                let p = g.snapshot.channel(parent).ok_or_else(|| not_found("category", parent))?;
                if p.kind != LiveChannelKind::Category {
                    return Err(PortError::Discord {
                        status: 400,
                        code: None,
                        message: "parent is not a category".to_owned(),
                    });
                }
            }
            *next += 1;
            let mut ch = LiveChannel {
                id: Id(*next),
                name: String::new(),
                kind: fields.kind.unwrap_or(LiveChannelKind::Known(ChannelKind::Text)),
                parent_id: None,
                position: 0,
                topic: None,
                nsfw: false,
                slowmode: 0,
                bitrate: None,
                user_limit: None,
                rtc_region: None,
                tags: Vec::new(),
                default_reaction: None,
                overwrites: Vec::new(),
            };
            FakeGuild::apply_channel_fields(&mut ch, fields, next);
            if fields.position.is_none() {
                ch.position = g.snapshot.channels.iter().map(|c| c.position).max().map_or(0, |m| m + 1);
            }
            let id = ch.id;
            g.snapshot.channels.push(ch);
            Ok(id)
        })
    }

    async fn update_channel(&self, guild: Id, channel: Id, fields: &ChannelFields, reason: &str) -> PortResult<()> {
        self.write("update_channel", guild, Some(channel), Some(reason), |g, next| {
            g.bot_can(Permission::ManageChannels)?;
            let has_news = g.snapshot.guild.features.contains("NEWS");
            let ch = g
                .snapshot
                .channels
                .iter_mut()
                .find(|c| c.id == channel)
                .ok_or_else(|| not_found("channel", channel))?;
            if let Some(k) = fields.kind
                && k != ch.kind
            {
                let text_news = |k: LiveChannelKind| {
                    matches!(k, LiveChannelKind::Known(ChannelKind::Text | ChannelKind::Announcement))
                };
                if !(text_news(k) && text_news(ch.kind) && has_news) {
                    return Err(PortError::Discord {
                        status: 400,
                        code: None,
                        message: "channel type can't be changed".to_owned(),
                    });
                }
            }
            FakeGuild::apply_channel_fields(ch, fields, next);
            Ok(())
        })
    }

    async fn delete_channel(&self, guild: Id, channel: Id, reason: &str) -> PortResult<()> {
        self.write("delete_channel", guild, Some(channel), Some(reason), |g, _| {
            g.bot_can(Permission::ManageChannels)?;
            g.snapshot.channel(channel).ok_or_else(|| not_found("channel", channel))?;
            g.snapshot.channels.retain(|c| c.id != channel);
            // Children of a deleted category move to the top level.
            for c in &mut g.snapshot.channels {
                if c.parent_id == Some(channel) {
                    c.parent_id = None;
                }
            }
            Ok(())
        })
    }

    async fn reorder_channels(&self, guild: Id, positions: &[ChannelPosition], reason: &str) -> PortResult<()> {
        self.write("reorder_channels", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageChannels)?;
            if positions.iter().filter(|p| p.parent_id.is_some()).count() > 1 {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(40009),
                    message: "only one parent_id change per request".to_owned(),
                });
            }
            for p in positions {
                let ch =
                    g.snapshot.channels.iter_mut().find(|c| c.id == p.id).ok_or_else(|| not_found("channel", p.id))?;
                ch.position = p.position;
                if let Some(parent) = p.parent_id {
                    ch.parent_id = parent;
                }
            }
            Ok(())
        })
    }

    async fn add_member_role(&self, guild: Id, user: Id, role: Id, reason: &str) -> PortResult<()> {
        self.write("add_member_role", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::ManageRoles)?;
            g.role_below_bot(role)?;
            let m = g.members.iter_mut().find(|m| m.id == user).ok_or_else(|| not_found("member", user))?;
            if !m.role_ids.contains(&role) {
                m.role_ids.push(role);
            }
            Ok(())
        })
    }

    async fn remove_member_role(&self, guild: Id, user: Id, role: Id, reason: &str) -> PortResult<()> {
        self.write("remove_member_role", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::ManageRoles)?;
            g.role_below_bot(role)?;
            let m = g.members.iter_mut().find(|m| m.id == user).ok_or_else(|| not_found("member", user))?;
            m.role_ids.retain(|r| *r != role);
            Ok(())
        })
    }

    async fn send_message(&self, channel: Id, content: &str) -> PortResult<Message> {
        let guild = self.guild_of_channel(channel)?;
        let bot = self.lock().user.clone();
        self.write("send_message", guild, Some(channel), None, |g, next| {
            *next += 1;
            let msg = Message {
                id: Id(*next),
                channel_id: channel,
                author_id: bot.id,
                author_name: bot.username.clone(),
                content: content.to_owned(),
                timestamp: "2026-10-19T12:00:00Z".to_owned(),
            };
            g.messages.entry(channel).or_default().push(msg.clone());
            Ok(msg)
        })
    }

    async fn read_messages(&self, channel: Id, limit: u8, before: Option<Id>) -> PortResult<Vec<Message>> {
        let guild = self.guild_of_channel(channel)?;
        self.read("read_messages", |st| {
            let g = st.guilds.get(&guild).ok_or(PortError::NotInGuild(guild))?;
            let all = g.messages.get(&channel).cloned().unwrap_or_default();
            Ok(all.into_iter().rev().filter(|m| before.is_none_or(|b| m.id < b)).take(usize::from(limit)).collect())
        })
    }

    async fn ban(&self, guild: Id, user: Id, _delete_message_seconds: u32, reason: &str) -> PortResult<()> {
        self.write("ban", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::BanMembers)?;
            g.members.retain(|m| m.id != user);
            if !g.bans.contains(&user) {
                g.bans.push(user);
            }
            Ok(())
        })
    }

    async fn unban(&self, guild: Id, user: Id, reason: &str) -> PortResult<()> {
        self.write("unban", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::BanMembers)?;
            g.bans.retain(|b| *b != user);
            Ok(())
        })
    }

    async fn kick(&self, guild: Id, user: Id, reason: &str) -> PortResult<()> {
        self.write("kick", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::KickMembers)?;
            let before = g.members.len();
            g.members.retain(|m| m.id != user);
            if g.members.len() == before {
                return Err(not_found("member", user));
            }
            Ok(())
        })
    }

    async fn timeout(&self, guild: Id, user: Id, until: Option<&str>, reason: &str) -> PortResult<()> {
        self.write("timeout", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::ModerateMembers)?;
            let m = g.members.iter_mut().find(|m| m.id == user).ok_or_else(|| not_found("member", user))?;
            m.timed_out = until.is_some();
            Ok(())
        })
    }
}

/// Ready-made servers for tests.
pub mod fixtures {
    use std::collections::BTreeSet;

    use guildsmith_core::snapshot::{
        BotInfo, Completeness, LiveChannel, LiveChannelKind, LiveGuild, LiveOverwrite, LiveRole, OverwriteKind,
        RoleTags, Snapshot,
    };
    use guildsmith_core::spec::ChannelKind;
    use guildsmith_core::{Id, Permission, PermissionSet};

    use super::FakeDiscord;
    use crate::ports::Member;

    pub const GUILD: Id = Id(1000);
    pub const OWNER: Id = Id(1);
    pub const BOT: Id = Id(2);
    pub const BOT_ROLE: Id = Id(9);
    pub const MODERATOR: Id = Id(10);
    pub const MEMBER_ROLE: Id = Id(11);
    pub const COMMUNITY: Id = Id(100);
    pub const GENERAL: Id = Id(101);
    pub const MOD_LOG: Id = Id(102);
    pub const LOBBY: Id = Id(103);
    pub const ALEX: Id = Id(50);
    pub const SAM: Id = Id(51);

    fn perms(list: &[Permission]) -> PermissionSet {
        list.iter().copied().collect()
    }

    fn role(id: Id, name: &str, position: i64, permissions: PermissionSet) -> LiveRole {
        LiveRole {
            id,
            name: name.to_owned(),
            color: 0,
            secondary_color: None,
            tertiary_color: None,
            hoist: false,
            mentionable: false,
            managed: false,
            permissions,
            position,
            tags: None,
        }
    }

    #[must_use]
    pub fn channel(id: Id, name: &str, kind: LiveChannelKind, parent: Option<Id>, position: i32) -> LiveChannel {
        LiveChannel {
            id,
            name: name.to_owned(),
            kind,
            parent_id: parent,
            position,
            topic: None,
            nsfw: false,
            slowmode: 0,
            bitrate: None,
            user_limit: None,
            rtc_region: None,
            tags: Vec::new(),
            default_reaction: None,
            overwrites: Vec::new(),
        }
    }

    /// "Guildsmith Lab": @everyone, a managed admin bot role, @Moderator, @Member; a Community
    /// category with #general and a private #mod-log; a top-level Lobby voice channel.
    #[must_use]
    pub fn lab() -> Snapshot {
        let everyone = role(
            GUILD,
            "@everyone",
            0,
            perms(&[
                Permission::ViewChannel,
                Permission::SendMessages,
                Permission::ReadMessageHistory,
                Permission::Connect,
                Permission::Speak,
            ]),
        );
        let mut bot_role = role(BOT_ROLE, "Guildsmith", 3, perms(&[Permission::Administrator]));
        bot_role.managed = true;
        bot_role.tags = Some(RoleTags { bot_id: Some(BOT), integration_id: None, premium_subscriber: false });
        let mut moderator = role(
            MODERATOR,
            "Moderator",
            2,
            perms(&[Permission::KickMembers, Permission::ManageMessages, Permission::ModerateMembers]),
        );
        moderator.color = 0x3B_A5_5D;
        moderator.hoist = true;
        let member = role(MEMBER_ROLE, "Member", 1, PermissionSet::EMPTY);

        let mut general = channel(GENERAL, "general", LiveChannelKind::Known(ChannelKind::Text), Some(COMMUNITY), 1);
        general.topic = Some("Talk about anything".to_owned());
        let mut mod_log = channel(MOD_LOG, "mod-log", LiveChannelKind::Known(ChannelKind::Text), Some(COMMUNITY), 2);
        mod_log.overwrites = vec![
            LiveOverwrite {
                id: GUILD,
                kind: OverwriteKind::Role,
                allow: PermissionSet::EMPTY,
                deny: perms(&[Permission::ViewChannel]),
            },
            LiveOverwrite {
                id: MODERATOR,
                kind: OverwriteKind::Role,
                allow: perms(&[Permission::ViewChannel]),
                deny: PermissionSet::EMPTY,
            },
        ];
        let mut lobby = channel(LOBBY, "Lobby", LiveChannelKind::Known(ChannelKind::Voice), None, 3);
        lobby.bitrate = Some(64_000);
        lobby.user_limit = Some(10);

        Snapshot {
            guild: LiveGuild {
                id: GUILD,
                name: "Guildsmith Lab".to_owned(),
                owner_id: OWNER,
                features: BTreeSet::new(),
                premium_tier: 0,
            },
            roles: vec![everyone, bot_role, moderator, member],
            channels: vec![
                channel(COMMUNITY, "Community", LiveChannelKind::Category, None, 0),
                general,
                mod_log,
                lobby,
            ],
            bot: BotInfo {
                user_id: BOT,
                role_ids: vec![BOT_ROLE],
                top_role_position: 3,
                permissions: PermissionSet::all_known(),
            },
            completeness: Completeness::Complete,
        }
    }

    /// The lab server plus every channel kind guildsmith reads: an announcement channel, a
    /// forum with tags, a stage, a channel type it doesn't manage, a gradient role color, and
    /// the `NEWS` feature. Mirrors the adapter's recorded fixtures.
    #[must_use]
    pub fn rich() -> Snapshot {
        let mut s = lab();
        s.guild.features.insert("NEWS".to_owned());
        if let Some(m) = s.roles.iter_mut().find(|r| r.id == MODERATOR) {
            m.secondary_color = Some(0xFF_00_00);
        }
        let mut news =
            channel(Id(104), "announcements", LiveChannelKind::Known(ChannelKind::Announcement), Some(COMMUNITY), 3);
        news.topic = Some("News".to_owned());
        let mut stage = channel(Id(105), "Town Hall", LiveChannelKind::Known(ChannelKind::Stage), None, 5);
        stage.bitrate = Some(64_000);
        stage.user_limit = Some(0);
        stage.rtc_region = Some("us-west".to_owned());
        let mut help = channel(Id(106), "help", LiveChannelKind::Known(ChannelKind::Forum), None, 6);
        help.topic = Some("Ask here".to_owned());
        help.tags = vec![
            guildsmith_core::snapshot::LiveTag {
                id: Id(201),
                name: "question".to_owned(),
                emoji: Some("❓".to_owned()),
                moderated: false,
            },
            guildsmith_core::snapshot::LiveTag { id: Id(202), name: "solved".to_owned(), emoji: None, moderated: true },
        ];
        help.default_reaction = Some("👍".to_owned());
        let directory = channel(Id(108), "directory", LiveChannelKind::Other(14), None, 8);
        s.channels.extend([news, stage, help, directory]);
        s
    }

    /// Members of the lab server.
    #[must_use]
    pub fn lab_members() -> Vec<Member> {
        let m = |id: Id, name: &str, nick: Option<&str>, roles: &[Id]| Member {
            id,
            username: name.to_owned(),
            display_name: None,
            nick: nick.map(str::to_owned),
            role_ids: roles.to_vec(),
            timed_out: false,
        };
        vec![
            m(OWNER, "owner", None, &[]),
            m(BOT, "guildsmith", None, &[BOT_ROLE]),
            m(ALEX, "alex", Some("Lexi"), &[MODERATOR]),
            m(SAM, "sam", None, &[MEMBER_ROLE]),
        ]
    }

    /// A server with only @everyone and the bot's role.
    #[must_use]
    pub fn empty(id: Id, name: &str) -> Snapshot {
        let mut s = lab();
        s.guild.id = id;
        name.clone_into(&mut s.guild.name);
        s.roles.retain(|r| r.id == GUILD || r.id == BOT_ROLE);
        for r in &mut s.roles {
            if r.id == GUILD {
                r.id = id;
            }
        }
        s.channels.clear();
        s
    }

    /// A fake holding the lab server.
    #[must_use]
    pub fn lab_fake() -> FakeDiscord {
        FakeDiscord::new(vec![(lab(), lab_members())])
    }
}
