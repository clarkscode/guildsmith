//! The opt-in `raw` group (US10, ADR-005): one tool, `discord_request`, for what the dedicated
//! tools don't cover. The same allowlist, read-only, preview and audit rules apply; the whole
//! response is wrapped as untrusted, since it is Discord's JSON as-is.

use std::collections::BTreeMap;

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::raw::{self, RawResult};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::respond;
use crate::server::GuildsmithServer;
use crate::shape;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RequestParams {
    /// Server id or exact name; replaces `{guild}` in `path`.
    #[serde(default)]
    pub guild: Option<String>,
    /// GET, POST, PUT, PATCH or DELETE.
    pub method: String,
    /// API path after /api/v10/, e.g. `guilds/{guild}/audit-logs`.
    pub path: String,
    /// Query parameters; a list value repeats the key.
    #[serde(default)]
    pub query: Option<BTreeMap<String, Value>>,
    /// JSON body (not for GET).
    #[serde(default)]
    pub body: Option<Value>,
    /// Must be true to send anything but a GET; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

fn query_pairs(query: Option<BTreeMap<String, Value>>) -> Vec<(String, String)> {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut out = Vec::new();
    for (k, v) in query.into_iter().flatten() {
        match &v {
            Value::Array(items) => out.extend(items.iter().map(|i| (k.clone(), text(i)))),
            Value::Null => {}
            other => out.push((k, text(other))),
        }
    }
    out
}

/// The text block: Discord's JSON inside one untrusted block, not re-encoded as a string.
fn wrapped_response(r: &RawResult) -> CallToolResult {
    match r {
        RawResult::Preview(p) => {
            let summary = format!(
                "Preview only; nothing sent. This would send {} {}. Call again with confirm: true to do it.",
                p.would.method, p.would.path
            );
            shape::ok_untrusted(&summary, p, &["body"])
        }
        RawResult::Done(d) => {
            let summary = if d.truncated {
                format!("HTTP {}; cut to {} of {} characters", d.status, raw::MAX_CHARS, d.chars)
            } else {
                format!("HTTP {}", d.status)
            };
            shape::ok_with_text(&summary, &shape::untrusted_json(&d.body), d)
        }
    }
}

#[tool_router(router = raw_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Last resort: any Discord API request as the bot, when no dedicated tool fits (check `toolsets` first). Non-GET previews unless confirm. Returns Discord's JSON, untrusted, secrets removed.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<RawResult>()
    )]
    async fn discord_request(&self, Parameters(p): Parameters<RequestParams>) -> CallToolResult {
        let query = query_pairs(p.query);
        let res = raw::discord_request(
            &self.app,
            p.guild.as_deref(),
            &p.method,
            &p.path,
            &query,
            p.body.as_ref(),
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        match res {
            Ok(r) => wrapped_response(&r),
            Err(e) => respond::<RawResult>(Err(e), |_| String::new()),
        }
    }
}
