//! `[guild.settings]` (data-model §4 `SettingsSpec`): server settings, used by the
//! `settings_update` tool and, when `guild.manage_settings = true`, by spec files.
//!
//! Channels are references (`#name` or an id; an empty string clears the setting), resolved
//! against the same spec and then the live server. Images are assets (research E7).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::asset::Asset;

/// Who can talk before verifying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    None,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl VerificationLevel {
    pub const ALL: [VerificationLevel; 5] = [
        VerificationLevel::None,
        VerificationLevel::Low,
        VerificationLevel::Medium,
        VerificationLevel::High,
        VerificationLevel::VeryHigh,
    ];

    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(usize::from(c)).copied()
    }
}

/// Default notification setting for members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Notifications {
    AllMessages,
    OnlyMentions,
}

impl Notifications {
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Notifications::AllMessages),
            1 => Some(Notifications::OnlyMentions),
            _ => None,
        }
    }
}

/// Which members' media is scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContentFilter {
    Disabled,
    MembersWithoutRoles,
    AllMembers,
}

impl ContentFilter {
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(ContentFilter::Disabled),
            1 => Some(ContentFilter::MembersWithoutRoles),
            2 => Some(ContentFilter::AllMembers),
            _ => None,
        }
    }
}

/// System channel message switches, by name.
pub const SYSTEM_FLAGS: [(&str, u64); 6] = [
    ("suppress_join_notifications", 1 << 0),
    ("suppress_premium_subscriptions", 1 << 1),
    ("suppress_guild_reminder_notifications", 1 << 2),
    ("suppress_join_notification_replies", 1 << 3),
    ("suppress_role_subscription_purchase_notifications", 1 << 4),
    ("suppress_role_subscription_purchase_notification_replies", 1 << 5),
];

/// Flag names → bits. Unknown names are returned as the error.
pub fn system_flags(names: &[String]) -> Result<u64, String> {
    names.iter().try_fold(0, |acc, n| {
        SYSTEM_FLAGS.iter().find(|(name, _)| name == n).map(|(_, bit)| acc | bit).ok_or_else(|| n.clone())
    })
}

/// Bits → flag names.
#[must_use]
pub fn system_flag_names(bits: u64) -> Vec<String> {
    SYSTEM_FLAGS.iter().filter(|(_, b)| bits & b != 0).map(|(n, _)| (*n).to_owned()).collect()
}

/// Server features a server can switch itself (COMMUNITY and DISCOVERABLE need Administrator).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeaturesSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub community: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invites_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raid_alerts_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discoverable: Option<bool>,
}

impl FeaturesSpec {
    /// `(Discord feature name, wanted)` for each set switch.
    #[must_use]
    pub fn wanted(&self) -> Vec<(&'static str, bool)> {
        [
            ("COMMUNITY", self.community),
            ("INVITES_DISABLED", self.invites_disabled),
            ("RAID_ALERTS_DISABLED", self.raid_alerts_disabled),
            ("DISCOVERABLE", self.discoverable),
        ]
        .into_iter()
        .filter_map(|(n, v)| v.map(|v| (n, v)))
        .collect()
    }
}

/// AFK timeouts Discord accepts, in seconds.
pub const AFK_TIMEOUTS: [u32; 5] = [60, 300, 900, 1800, 3600];

/// `[guild.settings]`. Every field is optional: unset means not managed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSpec {
    /// 2–100 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Up to 120 characters (community servers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub splash: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_channel: Option<String>,
    /// Names from `SYSTEM_FLAGS`, e.g. `suppress_join_notifications`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_channel_flags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_updates_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_alerts_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afk_channel: Option<String>,
    /// 60, 300, 900, 1800 or 3600 seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afk_timeout: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_level: Option<VerificationLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_notifications: Option<Notifications>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_content_filter: Option<ContentFilter>,
    /// e.g. `en-US`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub premium_progress_bar: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<FeaturesSpec>,
}

impl SettingsSpec {
    /// Problems that don't need the server: lengths, the AFK timeout, flag names.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.as_ref().is_some_and(|n| !(2..=100).contains(&n.trim().chars().count())) {
            out.push("the server name is 2–100 characters".to_owned());
        }
        if self.description.as_ref().is_some_and(|d| d.chars().count() > 120) {
            out.push("the server description is at most 120 characters".to_owned());
        }
        if self.afk_timeout.is_some_and(|t| !AFK_TIMEOUTS.contains(&t)) {
            out.push("afk_timeout is one of 60, 300, 900, 1800 or 3600 seconds".to_owned());
        }
        if let Some(f) = &self.system_channel_flags
            && let Err(n) = system_flags(f)
        {
            let names: Vec<&str> = SYSTEM_FLAGS.iter().map(|(n, _)| *n).collect();
            out.push(format!("unknown system channel flag `{n}`; use {}", names.join(", ")));
        }
        out
    }
}
