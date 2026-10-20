//! `spec_export` and `spec_validate`.

use std::path::Path;

use guildsmith_app::assets::{self, Caller};
use guildsmith_app::date;
use guildsmith_app::error::ServiceError;
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::{DiscordPort, SystemClock};
use guildsmith_app::services::apply::{self, ApplyReport, Progress};
use guildsmith_app::services::export;
use guildsmith_app::services::plan::{self, PlanOutput};
use guildsmith_app::services::validate::{self, ValidateOutput};
use guildsmith_app::writeback;
use guildsmith_core::planner::Risk;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ProgressNotificationParam};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::respond;
use crate::server::GuildsmithServer;
use crate::shape;

/// A spec file an MCP tool may read and write: under GUILDSMITH_FILE_ROOTS.
fn spec_file(policy: &Policy, file: &str) -> Result<std::path::PathBuf, ServiceError> {
    let roots = assets::allowed_roots(policy, Caller::Mcp { base: None });
    assets::resolve_file(file, None, &roots)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExportParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Also write the file here. Refuses to overwrite an existing file.
    #[serde(default)]
    pub path: Option<String>,
    /// Save emoji, stickers, sounds and images under this directory's assets/ (must be in
    /// GUILDSMITH_FILE_ROOTS); otherwise they are exported as CDN urls.
    #[serde(default)]
    pub assets_dir: Option<String>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PlanParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The spec file text (guild.toml).
    pub toml: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ApplyParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// The same spec text that was planned.
    pub toml: String,
    /// The plan_hash from spec_plan, after the user reviewed that plan.
    pub plan_hash: String,
    /// The spec file (in GUILDSMITH_FILE_ROOTS): created items' ids are written into it.
    #[serde(default)]
    pub path: Option<String>,
}

#[tool_router(router = spec_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Export a server as guild.toml text: structure, settings and every other kind, never secrets.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ExportResult>()
    )]
    async fn spec_export(&self, Parameters(p): Parameters<ExportParams>) -> CallToolResult {
        let res = async {
            let dir = p.assets_dir.as_deref().map(|d| assets::writable_dir(&self.app.policy, d)).transpose()?;
            let out =
                export::export_to(&self.app, p.guild.as_deref(), &date::today(&SystemClock), dir.as_deref()).await?;
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

    #[tool(
        description = "Compare guild.toml text with the live server and return the ordered changes and a plan_hash.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<PlanOutput>()
    )]
    async fn spec_plan(&self, Parameters(p): Parameters<PlanParams>) -> CallToolResult {
        let res = plan::plan(&self.app, p.guild.as_deref(), &p.toml).await;
        respond(res, |o| {
            if o.has_changes() {
                let risk = if o.risk == Risk::Destructive { "destructive" } else { "safe" };
                format!("{} change(s), {risk}, plan_hash {}", o.ops.len(), o.plan_hash)
            } else {
                "No changes.".to_owned()
            }
        })
    }

    #[tool(
        description = "Apply a plan the user reviewed: needs the same toml and the plan_hash spec_plan returned.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<ApplyReport>()
    )]
    async fn spec_apply(
        &self,
        Parameters(p): Parameters<ApplyParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        // The service reports progress synchronously; a forwarder turns it into notifications.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
        let forwarder = ctx.meta.get_progress_token().map(|token| {
            let peer = ctx.peer.clone();
            tokio::spawn(async move {
                while let Some(e) = rx.recv().await {
                    #[allow(clippy::cast_precision_loss)] // op counts are tiny
                    let param = ProgressNotificationParam::new(token.clone(), e.index as f64)
                        .with_total(e.total as f64)
                        .with_message(e.summary);
                    let _ = peer.notify_progress(param).await;
                }
            })
        });
        let mut sink = |e: &Progress| {
            let _ = tx.send(e.clone());
        };
        let file = match p.path.as_deref().map(|f| spec_file(&self.app.policy, f)).transpose() {
            Ok(f) => f,
            Err(e) => return respond::<ApplyReport>(Err(e), |_| String::new()),
        };
        let base = file.as_deref().and_then(Path::parent);
        let caller = Caller::Mcp { base };
        let mut res = apply::apply_in(&self.app, p.guild.as_deref(), &p.toml, &p.plan_hash, caller, &mut sink).await;
        drop(tx);
        if let Some(f) = forwarder {
            let _ = f.await;
        }
        if let (Ok(report), Some(file)) = (&mut res, &file)
            && let Err(e) = writeback::write_ids(file, &report.written_ids)
        {
            tracing::warn!(error = %e, "couldn't write ids back");
            report.written_ids.clear();
        }
        respond(res, |r| match &r.failed {
            None if !r.secrets.is_empty() => format!(
                "applied {} change(s). secrets holds new webhook URLs: give them only to the user, now; they aren't \
                 shown again",
                r.applied.len()
            ),
            None => format!("applied {} change(s)", r.applied.len()),
            Some(f) => format!(
                "stopped after {} change(s): `{}` failed ({}); {} skipped; plan again to see what's left",
                r.applied.len(),
                f.summary,
                f.error,
                r.skipped.len()
            ),
        })
    }
}
