//! The opt-in `expressions` group (US6): custom emoji, stickers and soundboard sounds, with
//! approximate slot counts.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::{DiscordPort, ExpressionKind};
use guildsmith_app::services::expressions::{
    self, ExpressionInput, ExpressionRow, ExpressionUpdate, ExpressionUpdated, ExpressionsList,
};
use guildsmith_app::services::live::Deleted;
use guildsmith_core::asset::Asset;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

const FIELDS: &[&str] = &["name", "description", "tags"];

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// emoji, sticker or sound; omit for all three.
    #[serde(default)]
    pub kind: Option<ExpressionKind>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// emoji, sticker or sound.
    pub kind: ExpressionKind,
    /// Emoji 2-32 (letters, digits, _); stickers 2-30; sounds 2-32.
    pub name: String,
    /// The file: {url}, {data} (base64) or {file} (under GUILDSMITH_FILE_ROOTS).
    pub asset: Asset,
    /// Emoji: roles that may use it (default everyone).
    #[serde(default)]
    pub roles: Vec<String>,
    /// Stickers: empty or 2-100 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// Stickers (required): an emoji or words for autocomplete, up to 200 characters.
    #[serde(default)]
    pub tags: Option<String>,
    /// Sounds: 0-1.
    #[serde(default)]
    pub volume: Option<f64>,
    /// Sounds: a unicode emoji or custom emoji id.
    #[serde(default)]
    pub emoji: Option<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// emoji, sticker or sound.
    pub kind: ExpressionKind,
    /// The item: name, :name: or id.
    pub item: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Emoji: roles that may use it.
    #[serde(default)]
    pub roles: Option<Vec<String>>,
    /// Stickers.
    #[serde(default)]
    pub description: Option<String>,
    /// Stickers.
    #[serde(default)]
    pub tags: Option<String>,
    /// Sounds: 0-1.
    #[serde(default)]
    pub volume: Option<f64>,
    /// Sounds: a unicode emoji or custom emoji id (empty clears).
    #[serde(default)]
    pub emoji: Option<String>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// emoji, sticker or sound.
    pub kind: ExpressionKind,
    /// The item: name, :name: or id.
    pub item: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = expressions_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "List custom emoji, stickers and sounds with slots used and approximately left.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ExpressionsList>()
    )]
    async fn expressions_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        let res = expressions::expressions_list(&self.app, p.guild.as_deref(), p.kind).await;
        wrapped(res, FIELDS, |l| {
            let slots: Vec<String> =
                l.slots.iter().map(|s| format!("{} {}/~{}", s.kind, s.used, s.approx_max)).collect();
            format!("{} item(s); {}", l.items.len(), slots.join(", "))
        })
    }

    #[tool(
        description = "Upload an emoji, sticker or soundboard sound; files over the limit or in the wrong format are refused first.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<ExpressionRow>()
    )]
    async fn expression_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let input = ExpressionInput {
            kind: p.kind,
            name: p.name,
            asset: p.asset,
            roles: p.roles,
            description: p.description,
            tags: p.tags,
            volume: p.volume,
            emoji: p.emoji,
        };
        let res = expressions::expression_create(&self.app, p.guild.as_deref(), &input, p.reason.as_deref()).await;
        wrapped(res, FIELDS, |e| format!("added {} {}", e.kind.name(), e.id))
    }

    #[tool(
        description = "Rename an emoji, sticker or sound, or change its roles, tags, description, volume or emoji.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ExpressionUpdated>()
    )]
    async fn expression_update(&self, Parameters(p): Parameters<UpdateParams>) -> CallToolResult {
        let input = ExpressionUpdate {
            name: p.name,
            roles: p.roles,
            description: p.description,
            tags: p.tags,
            volume: p.volume,
            emoji: p.emoji,
        };
        let res =
            expressions::expression_update(&self.app, p.guild.as_deref(), p.kind, &p.item, &input, p.reason.as_deref())
                .await;
        wrapped(
            res,
            FIELDS,
            |u| if u.changes.is_empty() { "nothing to change".to_owned() } else { u.changes.join(", ") },
        )
    }

    #[tool(
        description = "Delete an emoji, sticker or sound; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn expression_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res = expressions::expression_delete(
            &self.app,
            p.guild.as_deref(),
            p.kind,
            &p.item,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        respond(res, |o| outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }
}
