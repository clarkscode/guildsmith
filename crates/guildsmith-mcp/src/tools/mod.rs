//! Tool definitions. Each tool translates input → an app service call → shaped output.

pub(crate) mod channels;
pub(crate) mod guild;
pub(crate) mod members;
pub(crate) mod messages;
pub(crate) mod moderation;
pub(crate) mod perms;
pub(crate) mod roles;
pub(crate) mod spec;

use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use rmcp::model::{CallToolResult, MetaObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::shape;

/// The `guild` argument shared by every server-scoped tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub(crate) struct GuildParam {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
}

/// Maps a service result to a tool result: refusals become `refusal`, other errors `error`.
pub(crate) fn respond<T: Serialize>(
    res: Result<T, ServiceError>,
    summary: impl FnOnce(&T) -> String,
) -> CallToolResult {
    match res {
        Ok(v) => shape::ok(&summary(&v), &v),
        Err(ServiceError::Refused(r)) => shape::refusal(&r),
        Err(e) => {
            let kind = match &e {
                ServiceError::NotFound { .. } => "not_found",
                ServiceError::Ambiguous { .. } => "ambiguous",
                ServiceError::Invalid(_) => "invalid",
                ServiceError::Io(_) => "io",
                ServiceError::Port(_) => "discord",
                ServiceError::Refused(_) => unreachable!("handled above"),
            };
            let candidates = match &e {
                ServiceError::Ambiguous { candidates, .. } => serde_json::to_value(candidates).unwrap_or_default(),
                ServiceError::NotFound { suggestions, .. } => serde_json::to_value(suggestions).unwrap_or_default(),
                _ => serde_json::Value::Null,
            };
            shape::error(&e.to_string(), &json!({ "kind": kind, "candidates": candidates }))
        }
    }
}

/// `_meta` for hard deletes: Claude Code asks the user even in bypass mode (research R5).
pub(crate) fn requires_user_interaction() -> MetaObject {
    let mut meta = MetaObject::new();
    meta.0.insert("anthropic/requiresUserInteraction".to_owned(), serde_json::Value::Bool(true));
    meta
}

/// The summary for a destructive tool: a preview, or `done`'s text.
pub(crate) fn outcome_summary<T>(o: &Outcome<T>, done: impl FnOnce(&T) -> String) -> String {
    match o {
        Outcome::Preview(p) => format!(
            "Preview only; nothing changed. This would: {}. Call again with confirm: true to do it.",
            p.would.join("; ")
        ),
        Outcome::Done(v) => done(v),
    }
}
