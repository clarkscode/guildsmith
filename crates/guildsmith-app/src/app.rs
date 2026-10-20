//! The application context both interfaces share: the Discord port, the policy, the snapshot
//! cache and the clock.

use std::sync::Arc;

use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;

use crate::cache::SnapshotCache;
use crate::policy::Policy;
use crate::ports::{Clock, DiscordPort, PortResult, SystemClock};
use crate::secrets::WebhookTokens;
use crate::toolsets::ToolsetState;

/// What every service needs. Built once per process by the binary.
///
/// Shared immutably: `policy` can't change after startup, and of `toolsets` only the
/// unlocked set can grow (data-model §1).
pub struct App<P> {
    pub port: P,
    pub policy: Policy,
    pub cache: SnapshotCache,
    pub clock: Arc<dyn Clock>,
    pub toolsets: ToolsetState,
    /// Tokens of webhooks created in this process (never stored, logged or shown again).
    pub webhook_tokens: WebhookTokens,
}

impl<P: DiscordPort> App<P> {
    pub fn new(port: P, policy: Policy) -> Self {
        Self::with_clock(port, policy, Arc::new(SystemClock))
    }

    /// An app whose cache follows `clock` (tests control time with it).
    pub fn with_clock(port: P, policy: Policy, clock: Arc<dyn Clock>) -> Self {
        let toolsets = ToolsetState::new(&policy.toolsets, &policy.blocked, clock.as_ref());
        App { port, policy, cache: SnapshotCache::default(), clock, toolsets, webhook_tokens: WebhookTokens::default() }
    }

    /// A snapshot that may come from the cache (at most 60 seconds old).
    pub async fn snapshot(&self, guild: Id) -> PortResult<Snapshot> {
        let now = self.clock.now();
        if let Some(s) = self.cache.get(guild, now) {
            return Ok(s);
        }
        self.fresh_snapshot(guild).await
    }

    /// A snapshot fetched from Discord now, bypassing the cache (and refreshing it).
    pub async fn fresh_snapshot(&self, guild: Id) -> PortResult<Snapshot> {
        let s = self.port.snapshot(guild).await?;
        self.cache.put(guild, self.clock.now(), s.clone());
        Ok(s)
    }

    /// Call after any write to `guild`.
    pub fn invalidate(&self, guild: Id) {
        self.cache.invalidate(guild);
    }
}
