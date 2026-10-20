//! `EventsPort`: scheduled events and their interested users (US3, research E4).

use std::future::Future;

use guildsmith_core::Id;
use serde_json::Value;

use super::PortResult;
use super::messages::UserInfo;
pub use guildsmith_core::live::{EventInfo, EventLocation};

/// Event fields to create or change. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventFields {
    pub name: Option<String>,
    /// `Some(None)` clears it.
    #[allow(clippy::option_option)] // three states: leave alone, clear, set
    pub description: Option<Option<String>>,
    /// RFC 3339, UTC.
    pub start: Option<String>,
    pub end: Option<String>,
    pub location: Option<EventLocation>,
    /// 1 scheduled, 2 active, 3 completed, 4 canceled.
    pub status: Option<u8>,
    /// Discord's `recurrence_rule` object.
    pub recurrence: Option<Value>,
    /// A cover image as a data URI.
    pub image: Option<String>,
}

pub trait EventsPort: Send + Sync {
    /// Every event in the server, with interested counts.
    fn events(&self, guild: Id) -> impl Future<Output = PortResult<Vec<EventInfo>>> + Send;
    fn create_event(
        &self,
        guild: Id,
        fields: &EventFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<EventInfo>> + Send;
    fn update_event(
        &self,
        guild: Id,
        event: Id,
        fields: &EventFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<EventInfo>> + Send;
    fn delete_event(&self, guild: Id, event: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// Interested users, in pages after a user id (`limit` ≤ 100).
    fn event_users(
        &self,
        guild: Id,
        event: Id,
        after: Option<Id>,
        limit: u8,
    ) -> impl Future<Output = PortResult<Vec<UserInfo>>> + Send;
}
