//! `[[webhooks]]` (data-model §4). A webhook has no token or URL field: those are secrets and
//! never live in the spec file (research E8).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::asset::Asset;
use crate::id::Id;

/// `[[webhooks]]`: an incoming webhook. There is no `url` or `token` field, ever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookSpec {
    /// Written back by apply. Without it, the webhook is matched by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–80 characters, without "clyde" or "discord".
    pub name: String,
    /// The channel it posts to (`#name` or an id).
    pub channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Asset>,
}

impl WebhookSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let lower = self.name.to_lowercase();
        if !(1..=80).contains(&self.name.chars().count()) || lower.contains("clyde") || lower.contains("discord") {
            vec!["webhook names are 1–80 characters, without \"clyde\" or \"discord\"".to_owned()]
        } else {
            Vec::new()
        }
    }
}
