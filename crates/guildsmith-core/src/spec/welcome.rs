//! `[welcome_screen]` (data-model §4 `WelcomeScreenSpec`): what community servers show new
//! members, with up to 5 suggested channels.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One suggested channel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WelcomeChannelSpec {
    /// `#name` or an id.
    pub channel: String,
    /// 1–42 characters.
    pub description: String,
    /// A unicode emoji or a custom emoji id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

/// `[welcome_screen]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WelcomeScreenSpec {
    pub enabled: bool,
    /// Up to 140 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<WelcomeChannelSpec>,
}

impl WelcomeScreenSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.description.as_ref().is_some_and(|d| d.chars().count() > 140) {
            out.push("the welcome screen description is at most 140 characters".to_owned());
        }
        if self.channels.len() > 5 {
            out.push(format!("a welcome screen suggests at most 5 channels ({} given)", self.channels.len()));
        }
        for c in &self.channels {
            if !(1..=42).contains(&c.description.trim().chars().count()) {
                out.push(format!("welcome channel descriptions are 1–42 characters ({})", c.channel));
            }
        }
        out
    }
}
