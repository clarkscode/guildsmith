//! `AutomodPort`: Discord's auto-moderation rules (US4). Rules use ids for channels and roles.

use std::future::Future;

use guildsmith_core::Id;
use guildsmith_core::spec::automod::AutomodRule;

use super::PortResult;

pub trait AutomodPort: Send + Sync {
    fn automod_rules(&self, guild: Id) -> impl Future<Output = PortResult<Vec<AutomodRule<Id>>>> + Send;
    fn create_automod(
        &self,
        guild: Id,
        rule: &AutomodRule<Id>,
        reason: &str,
    ) -> impl Future<Output = PortResult<AutomodRule<Id>>> + Send;
    /// Replaces a rule's settings. The trigger type can't change.
    fn update_automod(
        &self,
        guild: Id,
        id: Id,
        rule: &AutomodRule<Id>,
        reason: &str,
    ) -> impl Future<Output = PortResult<AutomodRule<Id>>> + Send;
    fn delete_automod(&self, guild: Id, id: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
}
