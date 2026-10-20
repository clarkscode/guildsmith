//! Snapshot builders shared by the core tests.
#![allow(dead_code)]

use std::collections::BTreeSet;

use guildsmith_core::snapshot::{
    BotInfo, Completeness, LiveChannel, LiveChannelKind, LiveGuild, LiveOverwrite, LiveRole, OverwriteKind, Snapshot,
};
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission, PermissionSet};

pub const GUILD: Id = Id(1000);
pub const OWNER: Id = Id(1);
pub const BOT: Id = Id(2);

pub mod kinds;

pub fn perms(list: &[Permission]) -> PermissionSet {
    list.iter().copied().collect()
}

pub fn role(id: u64, name: &str, position: i64, permissions: PermissionSet) -> LiveRole {
    LiveRole {
        id: Id(id),
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

pub fn channel(id: u64, name: &str, kind: LiveChannelKind, parent: Option<u64>, position: i32) -> LiveChannel {
    LiveChannel {
        id: Id(id),
        name: name.to_owned(),
        kind,
        parent_id: parent.map(Id),
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

pub fn text(id: u64, name: &str, parent: Option<u64>, position: i32) -> LiveChannel {
    channel(id, name, LiveChannelKind::Known(ChannelKind::Text), parent, position)
}

pub fn overwrite(id: Id, kind: OverwriteKind, allow: PermissionSet, deny: PermissionSet) -> LiveOverwrite {
    LiveOverwrite { id, kind, allow, deny }
}

/// A small server: @everyone, @Moderator, @Member; a Community category with #general and
/// a private #mod-log; a top-level Lobby voice channel.
pub fn lab() -> Snapshot {
    let everyone = role(
        GUILD.0,
        "@everyone",
        0,
        perms(&[
            Permission::ViewChannel,
            Permission::SendMessages,
            Permission::ReadMessageHistory,
            Permission::Connect,
            Permission::Speak,
            Permission::AttachFiles,
        ]),
    );
    let moderator = role(10, "Moderator", 2, perms(&[Permission::KickMembers, Permission::ManageMessages]));
    let member = role(11, "Member", 1, PermissionSet::EMPTY);
    let mut mod_log = text(102, "mod-log", Some(100), 1);
    mod_log.overwrites = vec![
        overwrite(GUILD, OverwriteKind::Role, PermissionSet::EMPTY, perms(&[Permission::ViewChannel])),
        overwrite(Id(10), OverwriteKind::Role, perms(&[Permission::ViewChannel]), PermissionSet::EMPTY),
    ];
    Snapshot {
        guild: LiveGuild {
            id: GUILD,
            name: "Guildsmith Lab".to_owned(),
            owner_id: OWNER,
            features: BTreeSet::new(),
            premium_tier: 0,
        },
        roles: vec![everyone, moderator, member],
        channels: vec![
            channel(100, "Community", LiveChannelKind::Category, None, 0),
            text(101, "general", Some(100), 0),
            mod_log,
            channel(103, "Lobby", LiveChannelKind::Known(ChannelKind::Voice), None, 1),
        ],
        bot: BotInfo { user_id: BOT, role_ids: vec![Id(10)], top_role_position: 2, permissions: PermissionSet::EMPTY },
        completeness: Completeness::Complete,
    }
}
