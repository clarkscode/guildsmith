//! Server-level facts and the bot's own standing.

use std::collections::BTreeSet;

use guildsmith_core::snapshot::{BotInfo, LiveGuild, LiveRole};
use guildsmith_core::{Id, Permission, PermissionSet};
use twilight_model::guild::Guild;

use crate::ids;

pub(crate) fn live(g: &Guild) -> LiveGuild {
    LiveGuild {
        id: ids::from(g.id),
        name: g.name.clone(),
        owner_id: ids::from(g.owner_id),
        features: g
            .features
            .iter()
            .map(|f| std::borrow::Cow::<'static, str>::from(f.clone()).into_owned())
            .collect::<BTreeSet<_>>(),
        premium_tier: u8::from(g.premium_tier),
    }
}

/// Server-level permissions of a member with `role_ids`: `@everyone` plus every role, or all
/// permissions for the owner or an administrator.
pub(crate) fn bot_info(guild: &LiveGuild, roles: &[LiveRole], user_id: Id, role_ids: Vec<Id>) -> BotInfo {
    let mut permissions = roles.iter().find(|r| r.id == guild.id).map_or(PermissionSet::EMPTY, |r| r.permissions);
    let mut top_role_position = 0;
    for id in &role_ids {
        if let Some(r) = roles.iter().find(|r| r.id == *id) {
            permissions = permissions.union(r.permissions);
            top_role_position = top_role_position.max(r.position);
        }
    }
    if user_id == guild.owner_id || permissions.contains(Permission::Administrator) {
        permissions = PermissionSet::all_known().union(permissions);
    }
    BotInfo { user_id, role_ids, top_role_position, permissions }
}
