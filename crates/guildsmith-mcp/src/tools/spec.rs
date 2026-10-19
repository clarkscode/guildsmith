//! `spec_export` and `spec_validate`.

use std::path::Path;

use guildsmith_app::date;
use guildsmith_app::ports::{DiscordPort, SystemClock};
use guildsmith_app::services::export;
use guildsmith_app::services::validate::{self, ValidateOutput};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::respond;
use crate::server::GuildsmithServer;
use crate::shape;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExportParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Also write the file here. Refuses to overwrite an existing file.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ExportResult {
    pub toml: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub warnings: Vec<String>,
    pub unmanaged: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ValidateParams {
    /// The spec file text (guild.toml).
    pub toml: String,
}

#[tool_router(router = spec_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Export a server's roles, categories, channels and permissions as guild.toml text.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ExportResult>()
    )]
    async fn spec_export(&self, Parameters(p): Parameters<ExportParams>) -> CallToolResult {
        let res = async {
            let out = export::export(&self.app, p.guild.as_deref(), &date::today(&SystemClock)).await?;
            if let Some(path) = &p.path {
                export::write_file(Path::new(path), &out.toml, false)?;
            }
            Ok(ExportResult {
                toml: shape::cap_text(&out.toml),
                path: p.path,
                warnings: out.warnings,
                unmanaged: out.unmanaged,
            })
        }
        .await;
        respond(res, |r| match &r.path {
            Some(path) => format!("exported to {path} ({} lines)", r.toml.lines().count()),
            None => format!("exported {} lines", r.toml.lines().count()),
        })
    }

    #[tool(
        description = "Check guild.toml text against the schema and its references, without network access.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ValidateOutput>()
    )]
    async fn spec_validate(&self, Parameters(p): Parameters<ValidateParams>) -> CallToolResult {
        let out = validate::validate(&p.toml);
        respond(Ok(out), |o| {
            if o.valid {
                format!("valid ({} warning(s))", o.problems.len())
            } else {
                format!("{} problem(s)", o.problems.len())
            }
        })
    }
}
