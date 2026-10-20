//! `ModerationPort`: what the `moderation` group needs beyond v1's single ban, kick, timeout
//! and member-role calls (research E4): the ban list, bulk ban, prune, message deletion, the
//! audit log and incident actions.

use std::future::Future;

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;

use super::PortResult;

/// One ban. `username` and `reason` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BanEntry {
    pub user_id: Id,
    pub username: String,
    pub reason: Option<String>,
}

/// What a bulk ban request did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkBanResult {
    pub banned: Vec<Id>,
    pub failed: Vec<Id>,
}

/// One audit log entry. `reason` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditEntry {
    pub id: Id,
    /// Discord's numeric action type.
    pub action_type: u16,
    /// Who did it.
    pub user_id: Option<Id>,
    /// What it was done to.
    pub target_id: Option<Id>,
    pub reason: Option<String>,
    /// Changed keys, e.g. `name`, `permissions`.
    pub changes: Vec<String>,
}

/// The server's raid protections (incident actions). RFC 3339 times, `None` when off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Incidents {
    pub invites_disabled_until: Option<String>,
    pub dms_disabled_until: Option<String>,
}

pub trait ModerationPort: Send + Sync {
    /// The ban list, oldest first: `limit` ≤ 1000, then users after/before an id.
    fn bans(
        &self,
        guild: Id,
        limit: u16,
        before: Option<Id>,
        after: Option<Id>,
    ) -> impl Future<Output = PortResult<Vec<BanEntry>>> + Send;

    /// Bans at most 200 users in one request (needs `BAN_MEMBERS` and `MANAGE_GUILD`).
    fn bulk_ban(
        &self,
        guild: Id,
        users: &[Id],
        delete_message_seconds: u32,
        reason: &str,
    ) -> impl Future<Output = PortResult<BulkBanResult>> + Send;

    /// How many members a prune would remove: inactive for `days` (1–30), with no roles other
    /// than `roles` (which also must be among their roles).
    fn prune_count(&self, guild: Id, days: u8, roles: &[Id]) -> impl Future<Output = PortResult<u64>> + Send;

    /// Prunes. With `compute_count = false` (large servers) Discord returns no count.
    fn prune(
        &self,
        guild: Id,
        days: u8,
        roles: &[Id],
        compute_count: bool,
        reason: &str,
    ) -> impl Future<Output = PortResult<Option<u64>>> + Send;

    /// Deletes 2–100 messages newer than 14 days in one request.
    fn bulk_delete(&self, channel: Id, messages: &[Id], reason: &str) -> impl Future<Output = PortResult<()>> + Send;

    /// Deletes one message.
    fn delete_message(&self, channel: Id, message: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;

    /// Audit log entries, newest first: `limit` ≤ 100.
    fn audit_log(
        &self,
        guild: Id,
        action_type: Option<u16>,
        user: Option<Id>,
        before: Option<Id>,
        limit: u8,
    ) -> impl Future<Output = PortResult<Vec<AuditEntry>>> + Send;

    /// Sets the raid protections. Both times are sent (`None` turns one off).
    fn set_incidents(
        &self,
        guild: Id,
        incidents: &Incidents,
        reason: &str,
    ) -> impl Future<Output = PortResult<Incidents>> + Send;
}
