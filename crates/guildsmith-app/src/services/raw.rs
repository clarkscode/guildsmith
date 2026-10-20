//! The `raw` group (US10, FR-036–036d, research E2): any Discord request as the bot, within the
//! same limits as everything else.
//!
//! - The path must tie to a server on the allowlist (directly, or through one lookup of a
//!   channel, webhook or invite). Token routes, application commands, `OAuth2`, the gateway and
//!   creating servers are refused.
//! - Every method but GET is a write: refused in read-only mode, previewed until confirmed,
//!   sent with an audit reason, and the snapshot cache is cleared afterwards.
//! - Responses have tokens and webhook URLs removed and are cut to 50,000 characters; tools
//!   return them wrapped as untrusted. This is the one tool that returns Discord's JSON (ADR-005).

use guildsmith_core::Id;
use guildsmith_core::raw::{self, RawMethod, RawRequest, RawScope};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::App;
use crate::error::ServiceError;
use crate::policy::{Refusal, RefusalKind, audit_reason};
use crate::ports::{DiscordPort, PortError};
use crate::services::guilds;
use crate::services::live::invalid;

/// Responses are cut to this many characters.
pub const MAX_CHARS: usize = 50_000;

/// What would be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RawPreview {
    pub preview: bool,
    pub would: RawRequest,
    /// The server it acts on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild: Option<Id>,
}

/// Discord's answer, with secrets removed. `body` is untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RawResponse {
    pub status: u16,
    /// The JSON, as text, cut at 50,000 characters.
    pub body: String,
    pub truncated: bool,
    /// Length of the whole body, in characters.
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RawResult {
    Preview(RawPreview),
    Done(RawResponse),
}

fn unsupported(why: &str) -> ServiceError {
    Refusal::new(
        RefusalKind::Unsupported,
        format!("refused: {why}; nothing was sent"),
        Some("prefer a dedicated tool (toolsets lists them); raw requests must target an allowed server"),
    )
    .into()
}

async fn lookup<P: DiscordPort>(app: &App<P>, path: String, key: &[&str]) -> Result<Id, ServiceError> {
    let req = RawRequest { method: RawMethod::Get, path: path.clone(), query: Vec::new(), body: None };
    let reply = app.port.raw_request(&req, None).await.map_err(|e| match e {
        PortError::NotFound(_) => unsupported(&format!("{path} doesn't exist or the bot can't see it")),
        other => other.into(),
    })?;
    let mut v = &reply.json;
    for k in key {
        v = &v[*k];
    }
    v.as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| unsupported(&format!("guildsmith can't tie {path} to a server")))
}

/// The server a scope acts on (`None` for the bot's own account).
async fn guild_of<P: DiscordPort>(app: &App<P>, scope: &RawScope) -> Result<Option<Id>, ServiceError> {
    Ok(match scope {
        RawScope::Guild(g) => Some(*g),
        RawScope::Channel(c) => Some(lookup(app, format!("channels/{c}"), &["guild_id"]).await?),
        RawScope::Webhook(w) => Some(lookup(app, format!("webhooks/{w}"), &["guild_id"]).await?),
        RawScope::Invite(code) => Some(lookup(app, format!("invites/{code}"), &["guild", "id"]).await?),
        RawScope::SelfUser => None,
        RawScope::Refused(why) => return Err(unsupported(why)),
    })
}

/// Cuts `text` to `MAX_CHARS`, saying so.
fn cut(text: String) -> (String, bool, usize) {
    const NOTE: &str = "… [cut: the response was over 50,000 characters; narrow it with query parameters]";
    let chars = text.chars().count();
    if chars <= MAX_CHARS {
        return (text, false, chars);
    }
    let mut out: String = text.chars().take(MAX_CHARS - NOTE.chars().count()).collect();
    out.push_str(NOTE);
    (out, true, chars)
}

/// Sends any request to Discord's API as the bot. GETs run at once; other methods preview
/// `{method, path, query, body}` until `confirm`. `{guild}` in the path becomes the resolved
/// `guild`.
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn discord_request<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    method: &str,
    path: &str,
    query: &[(String, String)],
    body: Option<&Value>,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<RawResult, ServiceError> {
    let method: RawMethod = method.parse().map_err(|e: raw::UnknownMethod| invalid(e.to_string()))?;
    let path = if path.contains("{guild}") {
        path.replace("{guild}", &guilds::resolve(app, guild).await?.to_string())
    } else {
        path.to_owned()
    };
    let (path, mut q) = raw::normalize(&path);
    q.extend(query.iter().cloned());
    if method.is_write() {
        app.policy.ensure_writable()?;
    }
    if let Some(problem) = body.and_then(raw::body_problem) {
        return Err(unsupported(&problem));
    }
    if body.is_some() && !method.is_write() {
        return Err(invalid("a GET request has no body"));
    }
    let scope = raw::classify(&path, method);
    let guild = guild_of(app, &scope).await?;
    if let Some(g) = guild {
        app.policy.ensure_allowed(g)?;
    }
    let req = RawRequest { method, path, query: q, body: body.cloned() };
    if method.is_write() && !confirmed {
        return Ok(RawResult::Preview(RawPreview { preview: true, would: req, guild }));
    }
    let summary = format!("raw {} {}", req.method, req.path);
    let reply =
        app.port.raw_request(&req, method.is_write().then(|| audit_reason(&summary, reason)).as_deref()).await?;
    if method.is_write() {
        app.cache.clear();
    }
    let mut json = reply.json;
    raw::redact(&mut json);
    let (body, truncated, chars) = cut(serde_json::to_string(&json).unwrap_or_default());
    Ok(RawResult::Done(RawResponse { status: reply.status, body, truncated, chars }))
}
