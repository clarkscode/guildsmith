//! twilight models → guildsmith core types. twilight types never leave this crate.

pub(crate) mod channel;
pub(crate) mod guild;
pub(crate) mod overwrite;
pub(crate) mod role;

use guildsmith_core::PermissionSet;
use twilight_model::guild::Permissions;

/// Raw bits pass straight through, including flags twilight doesn't name (e.g. bit 48).
pub(crate) fn perms(p: Permissions) -> PermissionSet {
    PermissionSet::from_bits(p.bits())
}

pub(crate) fn to_perms(p: PermissionSet) -> Permissions {
    Permissions::from_bits_retain(p.bits())
}
