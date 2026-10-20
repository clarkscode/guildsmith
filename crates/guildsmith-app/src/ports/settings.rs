//! `SettingsPort`: server settings, onboarding, the welcome screen, templates, integrations,
//! the widget and the vanity URL (US7, research E4).

use std::collections::BTreeSet;
use std::future::Future;

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::PortResult;
pub use guildsmith_core::live::{
    GuildSettings, Onboarding, OnboardingOption, OnboardingPrompt, WelcomeChannel, WelcomeScreen,
};

/// Settings to change. `None` leaves a field alone; `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::option_option)] // three states: leave alone, clear, set
pub struct SettingsChange {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    /// Images as data URIs.
    pub icon: Option<Option<String>>,
    pub banner: Option<Option<String>>,
    pub splash: Option<Option<String>>,
    pub system_channel_id: Option<Option<Id>>,
    pub system_channel_flags: Option<u64>,
    pub rules_channel_id: Option<Option<Id>>,
    pub public_updates_channel_id: Option<Option<Id>>,
    pub safety_alerts_channel_id: Option<Option<Id>>,
    pub afk_channel_id: Option<Option<Id>>,
    pub afk_timeout: Option<u32>,
    pub verification_level: Option<u8>,
    pub default_notifications: Option<u8>,
    pub explicit_content_filter: Option<u8>,
    pub preferred_locale: Option<String>,
    pub premium_progress_bar: Option<bool>,
    /// The full list of features after the change (only the settable ones change).
    pub features: Option<BTreeSet<String>>,
}

/// A server template.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TemplateInfo {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub usage_count: u64,
    pub created_at: String,
    pub updated_at: String,
    /// The server changed since the last sync.
    pub dirty: bool,
    /// The template's server, with placeholder ids.
    pub serialized_source_guild: Value,
}

/// An integration (Twitch, `YouTube`, a bot application, …). Names are user-chosen: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct IntegrationInfo {
    pub id: Id,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WidgetSettings {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Vanity {
    pub code: Option<String>,
    pub uses: u32,
}

pub trait SettingsPort: Send + Sync {
    fn guild_settings(&self, guild: Id) -> impl Future<Output = PortResult<GuildSettings>> + Send;
    fn update_guild_settings(
        &self,
        guild: Id,
        change: &SettingsChange,
        reason: &str,
    ) -> impl Future<Output = PortResult<GuildSettings>> + Send;
    fn onboarding(&self, guild: Id) -> impl Future<Output = PortResult<Onboarding>> + Send;
    fn set_onboarding(
        &self,
        guild: Id,
        o: &Onboarding,
        reason: &str,
    ) -> impl Future<Output = PortResult<Onboarding>> + Send;
    fn welcome_screen(&self, guild: Id) -> impl Future<Output = PortResult<WelcomeScreen>> + Send;
    fn set_welcome_screen(
        &self,
        guild: Id,
        w: &WelcomeScreen,
        reason: &str,
    ) -> impl Future<Output = PortResult<WelcomeScreen>> + Send;
    fn templates(&self, guild: Id) -> impl Future<Output = PortResult<Vec<TemplateInfo>>> + Send;
    fn create_template(
        &self,
        guild: Id,
        name: &str,
        description: Option<&str>,
    ) -> impl Future<Output = PortResult<TemplateInfo>> + Send;
    fn sync_template(&self, guild: Id, code: &str) -> impl Future<Output = PortResult<TemplateInfo>> + Send;
    fn update_template(
        &self,
        guild: Id,
        code: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> impl Future<Output = PortResult<TemplateInfo>> + Send;
    fn delete_template(&self, guild: Id, code: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// Any public template, by code.
    fn get_template(&self, code: &str) -> impl Future<Output = PortResult<TemplateInfo>> + Send;
    fn integrations(&self, guild: Id) -> impl Future<Output = PortResult<Vec<IntegrationInfo>>> + Send;
    fn delete_integration(&self, guild: Id, id: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    fn widget(&self, guild: Id) -> impl Future<Output = PortResult<WidgetSettings>> + Send;
    fn vanity(&self, guild: Id) -> impl Future<Output = PortResult<Vanity>> + Send;
}
