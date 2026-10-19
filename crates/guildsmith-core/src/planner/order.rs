//! Op ordering (research R8):
//!
//! 1. Role creates and updates.
//! 2. One role reorder.
//! 3. Category creates and updates.
//! 4. Channel creates and updates, each with its own `parent_id`.
//! 5. One channel reorder (positions only).
//! 6. Deletes: channels, then categories, then roles.
//!
//! Within a phase, ops keep the spec's file order.

use super::ops::Op;

/// Where an op runs in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Roles,
    RoleOrder,
    Categories,
    Channels,
    ChannelOrder,
    DeleteChannels,
    DeleteCategories,
    DeleteRoles,
}

/// Sorts ops by phase, keeping file order within each phase.
#[must_use]
pub fn order(mut ops: Vec<(Phase, Op)>) -> Vec<Op> {
    ops.sort_by_key(|(phase, _)| *phase);
    ops.into_iter().map(|(_, op)| op).collect()
}
