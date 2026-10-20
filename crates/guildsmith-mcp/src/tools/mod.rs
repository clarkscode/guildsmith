//! Tool definitions. Each tool translates input → an app service call → shaped output.

pub(crate) mod automod;
pub(crate) mod channels;
pub(crate) mod events;
pub(crate) mod expressions;
pub(crate) mod guild;
pub(crate) mod invites;
pub(crate) mod members;
pub(crate) mod messages;
pub(crate) mod moderation;
pub(crate) mod perms;
pub(crate) mod polls;
pub(crate) mod raw;
pub(crate) mod roles;
pub(crate) mod settings;
pub(crate) mod spec;
pub(crate) mod threads;
pub(crate) mod voice;
pub(crate) mod webhooks;

use std::sync::atomic::Ordering;

use guildsmith_app::bulk::Progress;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::ports::DiscordPort;
use rmcp::RoleServer;
use rmcp::model::{CallToolResult, MetaObject, ProgressNotificationParam};
use rmcp::service::RequestContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::server::GuildsmithServer;
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

/// A result whose text block carries user-written fields, wrapped as untrusted data (FR-007).
pub(crate) fn wrapped<T: Serialize>(
    res: Result<T, ServiceError>,
    fields: &[&str],
    summary: impl FnOnce(&T) -> String,
) -> CallToolResult {
    match res {
        Ok(v) => shape::ok_untrusted(&summary(&v), &v, fields),
        Err(e) => respond::<T>(Err(e), |_| String::new()),
    }
}

/// `_meta` for hard deletes: Claude Code asks the user even in bypass mode (research R5).
pub(crate) fn requires_user_interaction() -> MetaObject {
    let mut meta = MetaObject::new();
    meta.0.insert("anthropic/requiresUserInteraction".to_owned(), serde_json::Value::Bool(true));
    meta
}

/// The summary for a destructive tool whose preview quotes user-written text: the details stay
/// in the wrapped block, never in the summary.
pub(crate) fn quiet_outcome_summary<T>(o: &Outcome<T>, done: impl FnOnce(&T) -> String) -> String {
    match o {
        Outcome::Preview(p) => format!(
            "Preview only; nothing changed. {} change(s) listed below. Call again with confirm: true to do it.",
            p.would.len()
        ),
        Outcome::Done(v) => done(v),
    }
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

/// Waits for a bulk tool's last progress notifications to go out.
pub(crate) struct Finish(Option<tokio::task::JoinHandle<()>>);

impl Finish {
    /// Drops the progress sender (closing the channel), then waits for the forwarder.
    pub(crate) async fn done<F>(self, progress: F) {
        drop(progress);
        if let Some(f) = self.0 {
            let _ = f.await;
        }
    }
}

/// Progress and cancellation for a bulk tool (research E10): MCP `notifications/progress` when
/// the client sent a progress token; a stop on MCP cancellation or the CLI's Ctrl-C.
pub(crate) fn bulk_hooks<P: DiscordPort + 'static>(
    server: &GuildsmithServer<P>,
    ctx: &RequestContext<RoleServer>,
    action: &'static str,
) -> (impl Fn(Progress) + Send + Sync + use<P>, impl Fn() -> bool + Send + Sync + use<P>, Finish) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
    let forwarder = ctx.meta.get_progress_token().map(|token| {
        let peer = ctx.peer.clone();
        tokio::spawn(async move {
            while let Some(p) = rx.recv().await {
                #[allow(clippy::cast_precision_loss)] // target counts are far below 2^52
                let param = ProgressNotificationParam::new(token.clone(), p.done as f64)
                    .with_total(p.total as f64)
                    .with_message(format!("{action}: {} of {}", p.done, p.total));
                let _ = peer.notify_progress(param).await;
            }
        })
    });
    let progress = move |p: Progress| {
        let _ = tx.send(p);
    };
    let ct = ctx.ct.clone();
    let stop = server.stop_flag();
    let cancelled = move || ct.is_cancelled() || stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed));
    (progress, cancelled, Finish(forwarder))
}
