//! Setup checks: `bot_preflight` for agents and `guildsmith doctor` for people (FR-020,
//! FR-033). Both use a fresh snapshot, never the cache (research R18).

use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::{DiscordPort, PortError};
use crate::preflight::{self, PreflightProblem};
use crate::services::{guilds, plan};

/// One problem, with what it means and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Finding {
    #[serde(flatten)]
    pub problem: PreflightProblem,
    pub message: String,
    pub fix: String,
}

impl From<PreflightProblem> for Finding {
    fn from(problem: PreflightProblem) -> Self {
        Finding { message: problem.to_string(), fix: problem.fix(), problem }
    }
}

/// What `bot_preflight` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PreflightReport {
    pub ok: bool,
    pub problems: Vec<Finding>,
}

impl PreflightReport {
    fn from_problems(problems: Vec<PreflightProblem>) -> Self {
        PreflightReport { ok: problems.is_empty(), problems: problems.into_iter().map(Finding::from).collect() }
    }
}

/// Checks the bot for `action`: `spec_apply` (with `toml`, the same checks apply runs), one
/// tool's name, or nothing for every enabled tool plus role order.
pub async fn bot_preflight<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    action: Option<&str>,
    toml: Option<&str>,
) -> Result<PreflightReport, ServiceError> {
    let action = action.map(str::trim).filter(|a| !a.is_empty());
    if let Some(a) = action
        && a != "spec_apply"
        && preflight::tool_needs(a).is_none()
    {
        let tools: Vec<&str> = app.policy.toolsets.iter().flat_map(|t| preflight::toolset_tools(*t)).copied().collect();
        return Err(ServiceError::Invalid(format!(
            "unknown action `{a}`; use spec_apply or a tool name: {}",
            tools.join(", ")
        )));
    }
    let spec = match (action, toml) {
        (Some("spec_apply"), Some(text)) => Some(plan::parse_spec(text)?),
        (Some("spec_apply"), None) => return Err(ServiceError::Invalid("action spec_apply needs `toml`".to_owned())),
        _ => None,
    };
    let id = guilds::resolve(app, guild).await?;
    let snapshot = app.fresh_snapshot(id).await?;
    let problems = match (action, spec) {
        (Some(_), Some(spec)) => preflight::check_plan(&snapshot, &plan::build(&spec, &snapshot)?),
        (Some(name), None) => preflight::check_tools(&snapshot, [name]),
        (None, _) => {
            let tools = app.policy.toolsets.iter().flat_map(|t| preflight::toolset_tools(*t)).copied();
            let mut all = preflight::check_tools(&snapshot, tools);
            all.extend(preflight::check_role_order(&snapshot));
            all
        }
    };
    Ok(PreflightReport::from_problems(problems))
}

/// How one doctor check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    /// Worth knowing, but doesn't fail the check run.
    Warn,
    Fail,
    /// Not run because an earlier check failed.
    Skipped,
}

/// One doctor check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Finding>,
}

impl Check {
    fn new(name: &str, status: Status, detail: impl Into<String>) -> Self {
        Check { name: name.to_owned(), status, detail: detail.into(), problems: Vec::new() }
    }
}

/// Everything `guildsmith doctor` checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DoctorReport {
    /// `true` when no check failed.
    pub ok: bool,
    pub checks: Vec<Check>,
}

fn problems_check(name: &str, pass: &str, problems: Vec<PreflightProblem>) -> Check {
    if problems.is_empty() {
        return Check::new(name, Status::Pass, pass);
    }
    let detail = format!("{} problem(s)", problems.len());
    Check { problems: problems.into_iter().map(Finding::from).collect(), ..Check::new(name, Status::Fail, detail) }
}

/// Checks the token, the allowlist, the bot's permissions for each enabled toolset, and role
/// order (FR-033). The token itself never appears in the report.
pub async fn doctor<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> DoctorReport {
    let mut checks = Vec::new();
    let token = match app.port.current_user().await {
        Ok(u) if u.bot => Check::new("token", Status::Pass, format!("valid bot token for {} ({})", u.username, u.id)),
        Ok(_) | Err(PortError::NotBotToken) => {
            Check::new("token", Status::Fail, "not a bot token: only bot application tokens are supported")
        }
        Err(PortError::Unauthorized) => {
            Check::new("token", Status::Fail, "token is invalid or was revoked; reset it in the developer portal")
        }
        Err(e) => Check::new("token", Status::Fail, e.to_string()),
    };
    let token_ok = token.status == Status::Pass;
    checks.push(token);

    checks.push(match &app.policy.allowlist {
        Some(list) => {
            let ids: Vec<String> = list.iter().map(ToString::to_string).collect();
            Check::new("allowlist", Status::Pass, format!("{} server(s) allowed: {}", list.len(), ids.join(", ")))
        }
        None => Check::new(
            "allowlist",
            Status::Warn,
            "no allowlist set; every server the bot is in is reachable (set GUILDSMITH_GUILDS)",
        ),
    });

    let names = ["server", "permissions", "role order"];
    if !token_ok {
        checks.extend(names.iter().map(|n| Check::new(n, Status::Skipped, "needs a working token")));
        return report(checks);
    }
    let snapshot = match guilds::resolve(app, guild).await {
        Ok(id) => app.fresh_snapshot(id).await.map_err(ServiceError::from),
        Err(e) => Err(e),
    };
    let snapshot = match snapshot {
        Ok(s) => {
            checks.push(Check::new("server", Status::Pass, format!("{} ({})", s.guild.name, s.guild.id)));
            s
        }
        Err(e) => {
            checks.push(Check::new("server", Status::Fail, e.to_string()));
            checks.extend(names[1..].iter().map(|n| Check::new(n, Status::Skipped, "needs the server")));
            return report(checks);
        }
    };
    for t in &app.policy.toolsets {
        let mut problems = preflight::check_tools(&snapshot, preflight::toolset_tools(*t).iter().copied());
        for p in &mut problems {
            if let PreflightProblem::MissingPermission { needed_by, .. } = p {
                *needed_by = format!("{needed_by} in the {t} toolset");
            }
        }
        let name = format!("permissions ({t})");
        checks.push(problems_check(&name, &format!("the bot has every permission the {t} toolset needs"), problems));
    }
    checks.push(problems_check(
        "role order",
        "the bot's highest role is above every role it manages",
        preflight::check_role_order(&snapshot),
    ));
    report(checks)
}

fn report(checks: Vec<Check>) -> DoctorReport {
    DoctorReport { ok: checks.iter().all(|c| c.status != Status::Fail), checks }
}
