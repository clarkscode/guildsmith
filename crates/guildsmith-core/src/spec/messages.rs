//! `[[messages]]`: fixed messages the bot posts and keeps as written, such as a rules post
//! (data-model §4). Ordinary conversation is not part of a spec (FR-043).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::Id;

pub const MAX_CONTENT: usize = 2000;
pub const MAX_EMBEDS: usize = 10;

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_false(b: &bool) -> bool {
    !*b
}

/// `[[messages]]`: a message the bot posted, kept as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixedMessageSpec {
    /// Written back by apply. Without it, the message is matched by exact content and embeds
    /// among the bot's last 100 messages in the channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// `#name` or an id.
    pub channel: String,
    /// At most 2000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Discord embed objects, at most 10.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<Value>,
    /// A Components V2 layout (an array of components). Excludes `content` and `embeds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components_v2: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Hide link previews.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_embeds: Option<bool>,
}

impl FixedMessageSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let content = self.content.as_deref().unwrap_or("");
        if self.components_v2.is_some() {
            if !content.is_empty() || !self.embeds.is_empty() {
                out.push("a components_v2 message can't also have content or embeds".to_owned());
            }
            if !self.components_v2.as_ref().is_some_and(Value::is_array) {
                out.push("components_v2 is an array of components".to_owned());
            }
        } else if content.is_empty() && self.embeds.is_empty() {
            out.push("a message needs content, embeds or components_v2".to_owned());
        }
        if content.chars().count() > MAX_CONTENT {
            out.push("content is at most 2000 characters".to_owned());
        }
        if self.embeds.len() > MAX_EMBEDS {
            out.push("at most 10 embeds".to_owned());
        }
        out
    }
}
