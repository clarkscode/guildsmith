//! `InvitesPort`: invites, their roles and target users (US3, research E3/E4).

use std::future::Future;

use guildsmith_core::Id;

use super::PortResult;
pub use guildsmith_core::live::InviteInfo;

/// An invite to create.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewInvite {
    pub max_age: u32,
    pub max_uses: u32,
    pub temporary: bool,
    /// Always a new code, rather than reusing a matching one.
    pub unique: bool,
    pub roles: Vec<Id>,
}

pub trait InvitesPort: Send + Sync {
    fn create_invite(
        &self,
        channel: Id,
        invite: &NewInvite,
        reason: &str,
    ) -> impl Future<Output = PortResult<InviteInfo>> + Send;
    /// Every invite in the server, with use counts.
    fn guild_invites(&self, guild: Id) -> impl Future<Output = PortResult<Vec<InviteInfo>>> + Send;
    fn delete_invite(&self, code: &str, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// The users an invite is limited to (empty: anyone).
    fn invite_target_users(&self, code: &str) -> impl Future<Output = PortResult<Vec<Id>>> + Send;
    /// Limits an invite to more users (at most 1000 per request).
    fn add_invite_target_users(&self, code: &str, users: &[Id]) -> impl Future<Output = PortResult<()>> + Send;
    fn remove_invite_target_users(&self, code: &str, users: &[Id]) -> impl Future<Output = PortResult<()>> + Send;
}
