//! Permission overwrites.

use guildsmith_core::snapshot::{LiveOverwrite, OverwriteKind};
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};

use crate::ids;

/// `None` for overwrite types Discord may add later.
pub(crate) fn live(o: &PermissionOverwrite) -> Option<LiveOverwrite> {
    let kind = match o.kind {
        PermissionOverwriteType::Role => OverwriteKind::Role,
        PermissionOverwriteType::Member => OverwriteKind::Member,
        _ => return None,
    };
    Some(LiveOverwrite { id: ids::from(o.id), kind, allow: super::perms(o.allow), deny: super::perms(o.deny) })
}
