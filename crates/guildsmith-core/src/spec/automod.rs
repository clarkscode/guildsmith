//! Auto-moderation rules (data-model §4 `AutomodRuleSpec`, research E4).
//!
//! One model for the `automod` tools and the spec file's `[[automod]]`: `R` is how channels and
//! roles are referred to (`String` references like `#mod-log` in a spec or tool input, `Id` once
//! resolved). Validation is pure and names Discord's limit in each problem.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::id::Id;

/// Keyword rules per server.
pub const MAX_KEYWORD_RULES: usize = 6;
pub const MAX_KEYWORDS: usize = 1000;
pub const MAX_KEYWORD_LEN: usize = 60;
pub const MAX_REGEX: usize = 10;
pub const MAX_REGEX_LEN: usize = 260;
pub const MAX_ALLOW: usize = 100;
pub const MAX_ALLOW_PRESET: usize = 1000;
pub const MAX_MENTIONS: u8 = 50;
pub const MAX_EXEMPT_ROLES: usize = 20;
pub const MAX_EXEMPT_CHANNELS: usize = 50;
/// 28 days.
pub const MAX_TIMEOUT: u32 = 2_419_200;
pub const MAX_BLOCK_MESSAGE: usize = 150;

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_true(b: &bool) -> bool {
    *b
}

const fn yes() -> bool {
    true
}

/// Discord's built-in word lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Profanity,
    SexualContent,
    Slurs,
}

impl Preset {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Preset::Profanity => 1,
            Preset::SexualContent => 2,
            Preset::Slurs => 3,
        }
    }

    #[must_use]
    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(Preset::Profanity),
            2 => Some(Preset::SexualContent),
            3 => Some(Preset::Slurs),
            _ => None,
        }
    }
}

/// Words, patterns and exceptions for keyword and member-profile rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Keywords {
    /// Words or phrases, with `*` wildcards (≤ 1000 × 60 characters).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Rust-flavored regular expressions (≤ 10 × 260 characters).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regex: Vec<String>,
    /// Exceptions (≤ 100).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

/// What a rule looks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Keyword(Keywords),
    /// Discord's spam detection.
    Spam,
    Preset {
        presets: Vec<Preset>,
        /// Exceptions (≤ 1000).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allow: Vec<String>,
    },
    MentionSpam {
        /// Unique role and user mentions per message (≤ 50).
        limit: u8,
        /// Also detect mention raids.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        raid_protection: bool,
    },
    /// Names and profile text of members.
    MemberProfile(Keywords),
}

impl Trigger {
    /// Discord's `trigger_type`.
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self {
            Trigger::Keyword(_) => 1,
            Trigger::Spam => 3,
            Trigger::Preset { .. } => 4,
            Trigger::MentionSpam { .. } => 5,
            Trigger::MemberProfile(_) => 6,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Trigger::Keyword(_) => "keyword",
            Trigger::Spam => "spam",
            Trigger::Preset { .. } => "preset",
            Trigger::MentionSpam { .. } => "mention_spam",
            Trigger::MemberProfile(_) => "member_profile",
        }
    }

    /// Discord's `event_type`: member updates for profile rules, messages otherwise.
    #[must_use]
    pub const fn event_type(&self) -> u8 {
        if matches!(self, Trigger::MemberProfile(_)) { 2 } else { 1 }
    }
}

/// What a rule does when it matches. `R` is a channel reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action<R> {
    /// Block the message, optionally telling the member why (≤ 150 characters).
    Block {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Post an alert in a channel.
    Alert { channel: R },
    /// Time the member out (seconds, ≤ 2419200). Keyword and mention-spam rules only.
    Timeout { duration: u32 },
    /// Stop the member interacting until they fix their profile. Member-profile rules only.
    BlockInteraction,
}

impl<R> Action<R> {
    /// Discord's action `type`.
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self {
            Action::Block { .. } => 1,
            Action::Alert { .. } => 2,
            Action::Timeout { .. } => 3,
            Action::BlockInteraction => 4,
        }
    }
}

/// One auto-moderation rule. `R` refers to channels and roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomodRule<R> {
    /// Written by export. Without it, the rule is matched by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters.
    pub name: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    pub trigger: Trigger,
    pub actions: Vec<Action<R>>,
    /// Roles the rule ignores (≤ 20).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exempt_roles: Vec<R>,
    /// Channels the rule ignores (≤ 50).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exempt_channels: Vec<R>,
}

impl<R> AutomodRule<R> {
    /// The same rule with its references mapped (e.g. names → ids).
    pub fn map_refs<S, E>(self, mut f: impl FnMut(R) -> Result<S, E>) -> Result<AutomodRule<S>, E> {
        Ok(AutomodRule {
            id: self.id,
            name: self.name,
            enabled: self.enabled,
            trigger: self.trigger,
            actions: self
                .actions
                .into_iter()
                .map(|a| {
                    Ok(match a {
                        Action::Block { message } => Action::Block { message },
                        Action::Alert { channel } => Action::Alert { channel: f(channel)? },
                        Action::Timeout { duration } => Action::Timeout { duration },
                        Action::BlockInteraction => Action::BlockInteraction,
                    })
                })
                .collect::<Result<_, E>>()?,
            exempt_roles: self.exempt_roles.into_iter().map(&mut f).collect::<Result<_, E>>()?,
            exempt_channels: self.exempt_channels.into_iter().map(&mut f).collect::<Result<_, E>>()?,
        })
    }
}

fn keywords(k: &Keywords, allow_max: usize, out: &mut Vec<String>) {
    if k.keywords.len() > MAX_KEYWORDS {
        out.push(format!("at most {MAX_KEYWORDS} keywords per rule ({} given)", k.keywords.len()));
    }
    if let Some(w) = k.keywords.iter().find(|w| w.chars().count() > MAX_KEYWORD_LEN || w.trim().is_empty()) {
        out.push(format!("keywords are 1–{MAX_KEYWORD_LEN} characters; `{w}` isn't"));
    }
    if k.regex.len() > MAX_REGEX {
        out.push(format!("at most {MAX_REGEX} regex patterns per rule ({} given)", k.regex.len()));
    }
    if let Some(r) = k.regex.iter().find(|r| r.chars().count() > MAX_REGEX_LEN || r.is_empty()) {
        out.push(format!("regex patterns are 1–{MAX_REGEX_LEN} characters; `{r}` isn't"));
    }
    if k.allow.len() > allow_max {
        out.push(format!("at most {allow_max} allowed words per rule ({} given)", k.allow.len()));
    }
    if k.keywords.is_empty() && k.regex.is_empty() {
        out.push("a keyword rule needs keywords or regex".to_owned());
    }
}

/// Everything wrong with one rule, each naming Discord's limit (research E4).
#[must_use]
pub fn problems<R>(rule: &AutomodRule<R>) -> Vec<String> {
    let mut out = Vec::new();
    if !(1..=100).contains(&rule.name.trim().chars().count()) {
        out.push("rule names are 1–100 characters".to_owned());
    }
    match &rule.trigger {
        Trigger::Keyword(k) | Trigger::MemberProfile(k) => keywords(k, MAX_ALLOW, &mut out),
        Trigger::Preset { presets, allow } => {
            if presets.is_empty() {
                out.push("a preset rule needs at least one preset (profanity, sexual_content, slurs)".to_owned());
            }
            if allow.len() > MAX_ALLOW_PRESET {
                out.push(format!("at most {MAX_ALLOW_PRESET} allowed words on a preset rule ({} given)", allow.len()));
            }
        }
        Trigger::MentionSpam { limit, .. } => {
            if *limit == 0 || *limit > MAX_MENTIONS {
                out.push(format!("the mention limit is 1–{MAX_MENTIONS}"));
            }
        }
        Trigger::Spam => {}
    }
    if rule.actions.is_empty() {
        out.push("a rule needs at least one action".to_owned());
    }
    let profile = matches!(rule.trigger, Trigger::MemberProfile(_));
    for a in &rule.actions {
        match a {
            Action::Block { message } => {
                if profile {
                    out.push("member-profile rules use block_interaction, not block".to_owned());
                }
                if message.as_ref().is_some_and(|m| m.chars().count() > MAX_BLOCK_MESSAGE) {
                    out.push(format!("a block message is at most {MAX_BLOCK_MESSAGE} characters"));
                }
            }
            Action::Timeout { duration } => {
                if !matches!(rule.trigger, Trigger::Keyword(_) | Trigger::MentionSpam { .. }) {
                    out.push("the timeout action is only allowed on keyword and mention-spam rules".to_owned());
                }
                if *duration == 0 || *duration > MAX_TIMEOUT {
                    out.push(format!("a timeout action lasts 1–{MAX_TIMEOUT} seconds (28 days)"));
                }
            }
            Action::BlockInteraction => {
                if !profile {
                    out.push("block_interaction is only for member-profile rules".to_owned());
                }
            }
            Action::Alert { .. } => {}
        }
    }
    if rule.exempt_roles.len() > MAX_EXEMPT_ROLES {
        out.push(format!("at most {MAX_EXEMPT_ROLES} exempt roles ({} given)", rule.exempt_roles.len()));
    }
    if rule.exempt_channels.len() > MAX_EXEMPT_CHANNELS {
        out.push(format!("at most {MAX_EXEMPT_CHANNELS} exempt channels ({} given)", rule.exempt_channels.len()));
    }
    out
}

/// Rule counts per trigger: keyword rules ≤ 6, every other type 1 each.
#[must_use]
pub fn count_problems<'a>(triggers: impl IntoIterator<Item = &'a Trigger>) -> Vec<String> {
    let mut counts = [0usize; 7];
    let mut names = [""; 7];
    for t in triggers {
        counts[usize::from(t.code())] += 1;
        names[usize::from(t.code())] = t.name();
    }
    let mut out = Vec::new();
    for (code, n) in counts.iter().enumerate() {
        let max = if code == 1 { MAX_KEYWORD_RULES } else { 1 };
        if *n > max {
            out.push(format!("at most {max} {} rule(s) per server ({n} would exist)", names[code]));
        }
    }
    out
}
