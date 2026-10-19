//! The snapshot cache (research R18, FR-031): one entry per server, kept for 60 seconds.
//!
//! Every write guildsmith makes clears it, and so does the `refresh` tool. `spec_apply` and
//! preflight never read from it: they always fetch a fresh snapshot (research R4).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;

pub const TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct SnapshotCache {
    entries: Mutex<BTreeMap<Id, (SystemTime, Snapshot)>>,
}

impl SnapshotCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Id, (SystemTime, Snapshot)>> {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The cached snapshot, if it is younger than the TTL at `now`.
    #[must_use]
    pub fn get(&self, guild: Id, now: SystemTime) -> Option<Snapshot> {
        let entries = self.lock();
        let (at, snapshot) = entries.get(&guild)?;
        // A clock that went backwards counts as expired.
        let fresh = now.duration_since(*at).is_ok_and(|age| age < TTL);
        fresh.then(|| snapshot.clone())
    }

    pub fn put(&self, guild: Id, now: SystemTime, snapshot: Snapshot) {
        self.lock().insert(guild, (now, snapshot));
    }

    /// Drops one server's entry (after a write to it).
    pub fn invalidate(&self, guild: Id) {
        self.lock().remove(&guild);
    }

    /// Drops every entry and returns the servers that had one.
    pub fn clear(&self) -> Vec<Id> {
        let mut entries = self.lock();
        let ids = entries.keys().copied().collect();
        entries.clear();
        ids
    }
}
