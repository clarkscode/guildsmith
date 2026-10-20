//! Op ordering (research R8):
//!
//! 1. Role creates and updates.
//! 2. One role reorder.
//! 3. Category creates and updates.
//! 4. Channel creates and updates, each with its own `parent_id`.
//! 5. One channel reorder (positions only).
//! 6. Server settings, which may refer to channels.
//! 7. Expressions, webhooks, automod, onboarding and the welcome screen, events, invites,
//!    fixed messages and pins (data-model §5).
//! 8. Deletes, in reverse dependency order: messages, invites, events, automod, webhooks,
//!    expressions, then channels, categories and roles.
//!
//! Within a phase, ops keep the spec's file order. Ids written back to the file come last, and
//! aren't ops (they change only the local file).

use super::ops::Op;

/// Where an op runs in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Roles,
    RoleOrder,
    Categories,
    Channels,
    ChannelOrder,
    Settings,
    Expressions,
    Webhooks,
    Automod,
    Onboarding,
    Events,
    Invites,
    Messages,
    DeleteMessages,
    DeleteInvites,
    DeleteEvents,
    DeleteAutomod,
    DeleteWebhooks,
    DeleteExpressions,
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
