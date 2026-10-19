//! `PlanService`: spec text + live server → a reviewable plan (FR-007, FR-012).

use guildsmith_core::planner::Risk;
use guildsmith_core::spec::GuildSpec;
use guildsmith_core::validate::{Severity, validate_text};
use guildsmith_core::{Id, Plan, diff};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::DiscordPort;
use crate::services::guilds;

/// One op as shown to people and agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct OpSummary {
    pub summary: String,
    pub risk: Risk,
}

/// What `spec_plan` / `guildsmith plan --json` return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanOutput {
    pub plan_hash: String,
    pub risk: Risk,
    pub ops: Vec<OpSummary>,
    pub unmanaged: Vec<String>,
    pub warnings: Vec<String>,
    pub estimated_requests: u32,
    #[serde(skip)]
    #[schemars(skip)]
    pub plan: Plan,
}

impl PlanOutput {
    fn from_plan(plan: Plan) -> Self {
        PlanOutput {
            plan_hash: plan.hash.clone(),
            risk: plan.risk,
            ops: plan.ops.iter().map(|o| OpSummary { summary: o.summary.clone(), risk: o.risk }).collect(),
            unmanaged: plan.unmanaged.clone(),
            warnings: plan.warnings.clone(),
            estimated_requests: plan.estimated_requests,
            plan,
        }
    }

    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.ops.is_empty()
    }
}

/// Parses and validates spec text; any error refuses with every problem and its location.
pub fn parse_spec(text: &str) -> Result<GuildSpec, ServiceError> {
    let report = validate_text(text);
    let errors: Vec<String> = report
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .map(|p| match p.location {
            Some(l) => format!("{l}: {}", p.message),
            None => p.message.clone(),
        })
        .collect();
    match report.spec {
        Some(spec) if errors.is_empty() => Ok(spec),
        _ => Err(ServiceError::Invalid(format!("the spec has errors:\n- {}", errors.join("\n- ")))),
    }
}

/// Plans `spec` against an already-fetched snapshot of `guild`.
pub(crate) fn build(spec: &GuildSpec, snapshot: &guildsmith_core::snapshot::Snapshot) -> Result<Plan, ServiceError> {
    diff(spec, snapshot).map_err(|e| ServiceError::Invalid(e.to_string()))
}

/// Plans spec text against a server. Uses the snapshot cache (research R18).
pub async fn plan<P: DiscordPort>(app: &App<P>, guild: Option<&str>, text: &str) -> Result<PlanOutput, ServiceError> {
    let spec = parse_spec(text)?;
    let id: Id = guilds::resolve(app, guild).await?;
    let snapshot = app.snapshot(id).await?;
    Ok(PlanOutput::from_plan(build(&spec, &snapshot)?))
}
