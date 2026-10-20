//! Webhook tokens held in memory (research E8, FR-030).
//!
//! A webhook's token is a credential: anyone with its URL can post as it. Tokens of webhooks
//! created in this process are kept here so `webhook_send` can use them. They are never
//! serialized, logged or written to disk, `Debug` shows no values, and the store is wiped when
//! the process ends.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use guildsmith_core::Id;

use crate::secret::Secret;

/// Webhook id → token, for webhooks created in this process.
#[derive(Default)]
pub struct WebhookTokens {
    tokens: Mutex<BTreeMap<Id, Secret>>,
}

impl WebhookTokens {
    pub fn insert(&self, webhook: Id, token: Secret) {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).insert(webhook, token);
    }

    #[must_use]
    pub fn get(&self, webhook: Id) -> Option<Secret> {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).get(&webhook).cloned()
    }

    pub fn remove(&self, webhook: Id) {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).remove(&webhook);
    }

    /// How many tokens are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for WebhookTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebhookTokens({} held, <redacted>)", self.len())
    }
}

impl Drop for WebhookTokens {
    fn drop(&mut self) {
        self.tokens.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

/// A webhook URL. Shown once, in the create result.
#[must_use]
pub fn webhook_url(id: Id, token: &Secret) -> String {
    format!("https://discord.com/api/webhooks/{id}/{}", token.expose())
}
