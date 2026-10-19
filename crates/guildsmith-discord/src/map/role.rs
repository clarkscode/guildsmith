//! Roles. Only the primary color is managed; gradient colors are read-only (research R2).

use guildsmith_core::snapshot::{LiveRole, RoleTags};
use twilight_model::guild::Role;

use crate::ids;

pub(crate) fn live(r: &Role) -> LiveRole {
    LiveRole {
        id: ids::from(r.id),
        name: r.name.clone(),
        color: r.colors.primary_color,
        secondary_color: r.colors.secondary_color,
        tertiary_color: r.colors.tertiary_color,
        hoist: r.hoist,
        mentionable: r.mentionable,
        managed: r.managed,
        permissions: super::perms(r.permissions),
        position: r.position,
        tags: r.tags.as_ref().map(|t| RoleTags {
            bot_id: t.bot_id.map(ids::from),
            integration_id: t.integration_id.map(ids::from),
            premium_subscriber: t.premium_subscriber,
        }),
    }
}
