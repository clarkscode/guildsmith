//! The opt-in `automod` group (US4): Discord's auto-moderation rules.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::automod::{self, RuleInput, RuleUpserted, RuleView};
use guildsmith_app::services::live::Deleted;
use guildsmith_core::spec::automod::{Action, Trigger};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{outcome_summary, respond};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpsertParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The rule to change (id or name); omit to create one.
    #[serde(default)]
    pub rule: Option<String>,
    /// 1-100 characters.
    #[serde(default)]
    pub name: Option<String>,
    /// Turning a rule off needs confirm.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// {keyword: {keywords, regex, allow}}, "spam", {preset: {presets, allow}}, {mention_spam: {limit, raid_protection}} or {member_profile: {...}}.
    #[serde(default)]
    pub trigger: Option<Trigger>,
    /// [{block: {message}}, {alert: {channel}}, {timeout: {duration}}, "block_interaction"].
    #[serde(default)]
    pub actions: Option<Vec<Action<String>>>,
    /// Roles the rule ignores (max 20).
    #[serde(default)]
    pub exempt_roles: Option<Vec<String>>,
    /// Channels the rule ignores (max 50).
    #[serde(default)]
    pub exempt_channels: Option<Vec<String>>,
    /// Needed to turn an enabled rule off; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Rule id or name.
    pub rule: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RulesOutput {
    pub rules: Vec<RuleView>,
}

#[tool_router(router = automod_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "List the server's auto-moderation rules with their triggers, actions and exemptions.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<RulesOutput>()
    )]
    async fn automod_rules_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        let res = automod::automod_rules_list(&self.app, p.guild.as_deref()).await.map(|rules| RulesOutput { rules });
        respond(res, |o| format!("{} rule(s)", o.rules.len()))
    }

    #[tool(
        description = "Create an auto-moderation rule, or change one; turning a rule off previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<RuleUpserted>>()
    )]
    async fn automod_rule_upsert(&self, Parameters(p): Parameters<UpsertParams>) -> CallToolResult {
        let input = RuleInput {
            name: p.name,
            enabled: p.enabled,
            trigger: p.trigger,
            actions: p.actions,
            exempt_roles: p.exempt_roles,
            exempt_channels: p.exempt_channels,
        };
        let res = automod::automod_rule_upsert(
            &self.app,
            p.guild.as_deref(),
            p.rule.as_deref(),
            &input,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| {
            outcome_summary(o, |u| {
                if u.created {
                    format!("created rule {}", u.rule.name)
                } else if u.changes.is_empty() {
                    "nothing to change".to_owned()
                } else {
                    format!("updated: {}", u.changes.join(", "))
                }
            })
        })
    }

    #[tool(
        description = "Delete an auto-moderation rule; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn automod_rule_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res =
            automod::automod_rule_delete(&self.app, p.guild.as_deref(), &p.rule, p.confirm, p.reason.as_deref()).await;
        respond(res, |o| outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }
}
