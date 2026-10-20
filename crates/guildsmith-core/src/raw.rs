//! Raw Discord requests (data-model §3, research E2): the request itself, and the pure path
//! classifier that ties a path to the server it acts on.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An HTTP method Discord's API uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum RawMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl RawMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RawMethod::Get => "GET",
            RawMethod::Post => "POST",
            RawMethod::Put => "PUT",
            RawMethod::Patch => "PATCH",
            RawMethod::Delete => "DELETE",
        }
    }

    /// Every method but GET changes something.
    #[must_use]
    pub const fn is_write(self) -> bool {
        !matches!(self, RawMethod::Get)
    }
}

impl fmt::Display for RawMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A method name that isn't one of GET, POST, PUT, PATCH, DELETE.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown method `{0}`; use GET, POST, PUT, PATCH or DELETE")]
pub struct UnknownMethod(pub String);

impl FromStr for RawMethod {
    type Err = UnknownMethod;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_ascii_uppercase().as_str() {
            "GET" => RawMethod::Get,
            "POST" => RawMethod::Post,
            "PUT" => RawMethod::Put,
            "PATCH" => RawMethod::Patch,
            "DELETE" => RawMethod::Delete,
            _ => return Err(UnknownMethod(s.to_owned())),
        })
    }
}

/// One request: `path` is relative to `/api/v10/`, without a leading slash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RawRequest {
    pub method: RawMethod,
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// What a raw path acts on (research E2). Lookups (channel, webhook, invite) are resolved to a
/// server by the app layer before the allowlist is checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RawScope {
    /// `guilds/{id}/…`, or `users/@me/guilds/{id}` (the bot leaving: destructive).
    Guild(crate::id::Id),
    /// `channels/{id}/…` and `stage-instances/{channel}`: the channel's server.
    Channel(crate::id::Id),
    /// `webhooks/{id}` without a token: the webhook's server.
    Webhook(crate::id::Id),
    /// `invites/{code}/…`: the invite's server.
    Invite(String),
    /// `users/@me…` reads: the bot's own account.
    SelfUser,
    /// Not allowed, and why.
    Refused(String),
}

/// A path relative to `/api/v10/`: no leading slash, no `api/` or version prefix, no query.
/// Returns the path and any query pairs that were in it.
#[must_use]
pub fn normalize(input: &str) -> (String, Vec<(String, String)>) {
    let mut p = input.trim();
    for prefix in ["https://discord.com", "http://discord.com", "/"] {
        p = p.strip_prefix(prefix).unwrap_or(p);
    }
    p = p.strip_prefix("api/").unwrap_or(p);
    if let Some(rest) = p.strip_prefix('v')
        && let Some((digits, tail)) = rest.split_once('/')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        p = tail;
    }
    let (path, query) = p.split_once('?').unwrap_or((p, ""));
    let pairs = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (k.to_owned(), v.to_owned()),
            None => (kv.to_owned(), String::new()),
        })
        .collect();
    (path.trim_end_matches('/').to_owned(), pairs)
}

fn refused(why: &str) -> RawScope {
    RawScope::Refused(why.to_owned())
}

/// Ties a (normalized) path to the server it acts on, or refuses it (research E2).
#[must_use]
pub fn classify(path: &str, method: RawMethod) -> RawScope {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let id = |s: &str| s.parse::<crate::id::Id>().ok();
    match segs.as_slice() {
        [] => refused("an empty path"),
        ["guilds"] if method == RawMethod::Post => {
            refused("bots can't create servers; use template_import to turn a template into a spec file")
        }
        ["guilds", g, ..] => id(g).map_or_else(
            || refused("guildsmith can't tie this path to a server on the allowlist (use the server's id)"),
            RawScope::Guild,
        ),
        ["channels" | "stage-instances", c, ..] => id(c).map_or_else(|| refused("not a channel id"), RawScope::Channel),
        ["webhooks", _, _, ..] => {
            refused("webhook token routes are refused so tokens stay out of agent traffic; use the webhooks tools")
        }
        ["webhooks", w] => id(w).map_or_else(|| refused("not a webhook id"), RawScope::Webhook),
        ["invites", code, ..] if !code.is_empty() => RawScope::Invite((*code).to_owned()),
        ["users", "@me", "guilds", g] if method == RawMethod::Delete => {
            id(g).map_or_else(|| refused("not a server id"), RawScope::Guild)
        }
        ["users", "@me", ..] if method == RawMethod::Get => RawScope::SelfUser,
        ["users", "@me", ..] => refused("guildsmith doesn't change the bot's own account"),
        ["applications", .., "commands"] | ["applications", .., "commands", _] => {
            refused("guildsmith doesn't manage application commands; it isn't a bot framework")
        }
        ["applications", ..] => refused("application routes aren't tied to a server"),
        ["oauth2", ..] => refused("OAuth2 routes are for user tokens; guildsmith only uses a bot token"),
        [g, ..] if g.starts_with("gateway") => refused("the gateway is out of scope; guildsmith is REST only"),
        _ => refused("guildsmith can't tie this path to a server on the allowlist"),
    }
}

/// Whether `s` is a webhook URL (it carries the webhook's token).
#[must_use]
pub fn is_webhook_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let Some(rest) = lower.split_once("://").map(|(_, r)| r) else { return false };
    let host_ok = ["discord.com/", "discordapp.com/", "canary.discord.com/", "ptb.discord.com/"]
        .iter()
        .any(|h| rest.starts_with(h));
    host_ok && rest.contains("/webhooks/")
}

pub const REDACTED: &str = "<redacted>";

/// Removes secrets from response JSON: every `token` field, and webhook URLs anywhere.
pub fn redact(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            map.remove("token");
            for child in map.values_mut() {
                redact(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        Value::String(s) if is_webhook_url(s) => REDACTED.clone_into(s),
        _ => {}
    }
}

/// Why a request body can't be sent: it holds an `Authorization` header or something shaped
/// like a token.
#[must_use]
pub fn body_problem(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    match v {
        Value::Object(map) => map.iter().find_map(|(k, child)| {
            if k.eq_ignore_ascii_case("authorization") {
                Some("the body carries an Authorization header".to_owned())
            } else {
                body_problem(child)
            }
        }),
        Value::Array(items) => items.iter().find_map(body_problem),
        Value::String(s) if is_webhook_url(s) => Some("the body carries a webhook URL".to_owned()),
        Value::String(s) if looks_like_bot_token(s) => {
            Some("the body carries something shaped like a bot token".to_owned())
        }
        _ => None,
    }
}

/// Three dot-separated base64url parts, the first a user id: a bot token's shape.
#[must_use]
pub fn looks_like_bot_token(s: &str) -> bool {
    let parts: Vec<&str> = s.trim().trim_start_matches("Bot ").split('.').collect();
    let b64 = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    parts.len() == 3
        && parts[0].len() >= 20
        && parts[1].len() >= 5
        && parts[2].len() >= 20
        && parts.iter().all(|p| b64(p))
}
