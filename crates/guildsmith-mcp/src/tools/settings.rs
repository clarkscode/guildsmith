//! The opt-in `settings` group (US7): server settings, onboarding, the welcome screen,
//! templates (and importing one into a spec file), and integrations.

use std::path::Path;

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::Deleted;
use guildsmith_app::services::settings::{
    self, SettingsSection, SettingsUpdated, SettingsView, TemplateAction, TemplateImport, Templates, WelcomeUpdated,
};
use guildsmith_core::spec::onboarding::OnboardingSpec;
use guildsmith_core::spec::settings::SettingsSpec;
use guildsmith_core::spec::welcome::WelcomeScreenSpec;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// general, onboarding, welcome_screen, widget, integrations or vanity; omit for all.
    #[serde(default)]
    pub section: Option<SettingsSection>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channels as #name or id (empty clears); images as {url}, {data} or {file}; features {community, invites_disabled, raid_alerts_disabled, discoverable}.
    #[serde(flatten)]
    pub settings: SettingsSpec,
    /// Must be true to act; otherwise returns a preview of old -> new.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct OnboardingParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The whole onboarding; it replaces what is there.
    #[serde(flatten)]
    pub onboarding: OnboardingSpec,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct WelcomeParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    #[serde(flatten)]
    pub welcome_screen: WelcomeScreenSpec,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TemplatesParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// list, create, sync, update, delete (needs confirm), or use (refused: bots can't create servers).
    pub action: TemplateAction,
    /// Template name (create, update).
    #[serde(default)]
    pub name: Option<String>,
    /// Template description (create, update).
    #[serde(default)]
    pub description: Option<String>,
    /// For delete: must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ImportParams {
    /// A template code or link (discord.new/... or discord.com/template/...).
    pub code_or_url: String,
    /// Also write the file here. Refuses to overwrite an existing file.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IntegrationParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Integration id or name.
    pub integration: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = settings_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Read server settings, onboarding, the welcome screen, widget, integrations or vanity URL.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<SettingsView>()
    )]
    async fn settings_get(&self, Parameters(p): Parameters<GetParams>) -> CallToolResult {
        let res = settings::settings_get(&self.app, p.guild.as_deref(), p.section).await;
        wrapped(res, &["name", "description", "title", "account"], |_| "settings".to_owned())
    }

    #[tool(
        description = "Change server settings (name, description, images, channels, verification, notifications, filters, AFK, features); previews old -> new unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<SettingsUpdated>>()
    )]
    async fn settings_update(&self, Parameters(p): Parameters<UpdateParams>) -> CallToolResult {
        let res =
            settings::settings_update(&self.app, p.guild.as_deref(), &p.settings, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| {
            outcome_summary(o, |u| {
                if u.changes.is_empty() {
                    "nothing to change".to_owned()
                } else {
                    format!("changed: {}", u.changes.join("; "))
                }
            })
        })
    }

    #[tool(
        description = "Replace the onboarding (prompts that give roles and channels, default channels); previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<OnboardingSpec>>()
    )]
    async fn onboarding_update(&self, Parameters(p): Parameters<OnboardingParams>) -> CallToolResult {
        let res =
            settings::onboarding_update(&self.app, p.guild.as_deref(), &p.onboarding, p.confirm, p.reason.as_deref())
                .await;
        respond(res, |o| outcome_summary(o, |x| format!("onboarding set: {} prompt(s)", x.prompts.len())))
    }

    #[tool(
        description = "Set the welcome screen: description and up to 5 suggested channels.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<WelcomeUpdated>()
    )]
    async fn welcome_screen_update(&self, Parameters(p): Parameters<WelcomeParams>) -> CallToolResult {
        let res =
            settings::welcome_screen_update(&self.app, p.guild.as_deref(), &p.welcome_screen, p.reason.as_deref())
                .await;
        respond(res, |w| if w.changed { "welcome screen updated".to_owned() } else { "already that way".to_owned() })
    }

    #[tool(
        description = "List, create, sync, update or delete the server's template (one per server).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Templates>>()
    )]
    async fn templates(&self, Parameters(p): Parameters<TemplatesParams>) -> CallToolResult {
        let res = settings::templates(
            &self.app,
            p.guild.as_deref(),
            p.action,
            p.name.as_deref(),
            p.description.as_deref(),
            p.confirm,
        )
        .await;
        respond(res, |o| {
            outcome_summary(o, |t| t.done.clone().unwrap_or_else(|| format!("{} template(s)", t.templates.len())))
        })
    }

    #[tool(
        description = "Turn a template (code or link) into guild.toml text to review and plan; never creates a server.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<TemplateImport>()
    )]
    async fn template_import(&self, Parameters(p): Parameters<ImportParams>) -> CallToolResult {
        let res = settings::template_import(&self.app, &p.code_or_url, p.path.as_deref().map(Path::new)).await;
        respond(res, |t| match &t.path {
            Some(path) => format!("imported {} to {path}", t.template),
            None => format!("imported {} ({} lines)", t.template, t.toml.lines().count()),
        })
    }

    #[tool(
        description = "Remove an integration (and its bot, roles and webhooks); previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn integration_remove(&self, Parameters(p): Parameters<IntegrationParams>) -> CallToolResult {
        let res =
            settings::integration_remove(&self.app, p.guild.as_deref(), &p.integration, p.confirm, p.reason.as_deref())
                .await;
        respond(res, |o| outcome_summary(o, |d| format!("removed {}", d.deleted)))
    }
}
