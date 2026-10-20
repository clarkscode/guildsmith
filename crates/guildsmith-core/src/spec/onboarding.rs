//! `[onboarding]` (data-model §4 `OnboardingSpec`): the questions new members answer and the
//! default channels they see. Roles and channels are references (`@Role`, `#channel` or ids).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::id::Id;

/// How new members get channels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingMode {
    /// Only the default channels count toward the requirements.
    #[default]
    DefaultChannels,
    /// Channels from prompts count too.
    Advanced,
}

/// How a prompt is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    #[default]
    MultipleChoice,
    Dropdown,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_false(b: &bool) -> bool {
    !*b
}

const fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_true(b: &bool) -> bool {
    *b
}

/// One answer to a prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–50 characters.
    pub title: String,
    /// Up to 100 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A unicode emoji or a custom emoji id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
}

/// One onboarding question.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters.
    pub title: String,
    #[serde(default)]
    pub kind: PromptKind,
    #[serde(default, skip_serializing_if = "is_false")]
    pub single_select: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Shown during onboarding (otherwise only in Channels & Roles).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub in_onboarding: bool,
    /// 1–50 options.
    pub options: Vec<OptionSpec>,
}

/// `[onboarding]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OnboardingSpec {
    pub enabled: bool,
    #[serde(default)]
    pub mode: OnboardingMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_channels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<PromptSpec>,
}

/// Onboarding needs this many default channels when enabled…
pub const MIN_DEFAULT_CHANNELS: usize = 7;
/// …of which this many `@everyone` can send messages in.
pub const MIN_SENDABLE: usize = 5;

impl OnboardingSpec {
    /// Problems that don't need the server: counts and lengths.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.prompts.len() > 15 {
            out.push("at most 15 onboarding prompts".to_owned());
        }
        for p in &self.prompts {
            if !(1..=100).contains(&p.title.trim().chars().count()) {
                out.push("prompt titles are 1–100 characters".to_owned());
            }
            if !(1..=50).contains(&p.options.len()) {
                out.push(format!("the prompt `{}` needs 1–50 options", p.title));
            }
            for o in &p.options {
                if !(1..=50).contains(&o.title.trim().chars().count()) {
                    out.push(format!("option titles are 1–50 characters (`{}`)", o.title));
                }
                if o.description.as_ref().is_some_and(|d| d.chars().count() > 100) {
                    out.push(format!("option descriptions are at most 100 characters (`{}`)", o.title));
                }
                if o.roles.is_empty() && o.channels.is_empty() {
                    out.push(format!("the option `{}` gives no roles or channels", o.title));
                }
            }
        }
        if self.enabled && self.default_channels.len() < MIN_DEFAULT_CHANNELS {
            out.push(format!(
                "an enabled onboarding needs at least {MIN_DEFAULT_CHANNELS} default channels ({} given)",
                self.default_channels.len()
            ));
        }
        out
    }
}
