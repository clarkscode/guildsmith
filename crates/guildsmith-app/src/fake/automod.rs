//! `AutomodPort` for the fake: Discord's per-type rule counts and immutable trigger types.

use std::collections::BTreeMap;

use guildsmith_core::spec::automod::{self, AutomodRule};
use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, next_id, not_found};
use crate::ports::{AutomodPort, PortError, PortResult};

/// The fake's rules for one server.
#[derive(Debug, Clone, Default)]
pub struct AutomodState {
    pub rules: BTreeMap<Id, AutomodRule<Id>>,
}

fn bad(message: String) -> PortError {
    PortError::Discord { status: 400, code: Some(50035), message }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl AutomodPort for FakeDiscord {
    async fn automod_rules(&self, guild: Id) -> PortResult<Vec<AutomodRule<Id>>> {
        self.read_guild("automod_rules", guild, None, |g| {
            g.bot_can(Permission::ManageGuild)?;
            Ok(g.res.peek::<AutomodState>().map(|s| s.rules.values().cloned().collect()).unwrap_or_default())
        })
    }

    async fn create_automod(&self, guild: Id, rule: &AutomodRule<Id>, reason: &str) -> PortResult<AutomodRule<Id>> {
        self.write("create_automod", guild, None, Some(reason), |g, next| {
            g.bot_can(Permission::ManageGuild)?;
            if let Some(p) = automod::problems(rule).into_iter().next() {
                return Err(bad(p));
            }
            let s = g.res.get::<AutomodState>();
            let mut triggers: Vec<_> = s.rules.values().map(|r| &r.trigger).collect();
            triggers.push(&rule.trigger);
            if let Some(p) = automod::count_problems(triggers).into_iter().next() {
                return Err(PortError::Discord { status: 400, code: Some(30035), message: p });
            }
            let created = AutomodRule { id: Some(next_id(next)), ..rule.clone() };
            s.rules.insert(created.id.expect("set"), created.clone());
            Ok(created)
        })
    }

    async fn update_automod(
        &self,
        guild: Id,
        id: Id,
        rule: &AutomodRule<Id>,
        reason: &str,
    ) -> PortResult<AutomodRule<Id>> {
        self.write("update_automod", guild, Some(id), Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            if let Some(p) = automod::problems(rule).into_iter().next() {
                return Err(bad(p));
            }
            let s = g.res.get::<AutomodState>();
            let current = s.rules.get_mut(&id).ok_or_else(|| not_found("automod rule", id))?;
            if current.trigger.code() != rule.trigger.code() {
                return Err(bad("a rule's trigger type can't change".to_owned()));
            }
            *current = AutomodRule { id: Some(id), ..rule.clone() };
            Ok(current.clone())
        })
    }

    async fn delete_automod(&self, guild: Id, id: Id, reason: &str) -> PortResult<()> {
        self.write("delete_automod", guild, Some(id), Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            g.res.get::<AutomodState>().rules.remove(&id).map(|_| ()).ok_or_else(|| not_found("automod rule", id))
        })
    }
}
