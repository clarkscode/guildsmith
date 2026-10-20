//! The raw request path (research E2/E3): any method and path, sent through twilight's
//! `RequestBuilder::raw` and `Client::request`, so it shares the client's auth and rate
//! limiter. The `ext` modules and the `raw` group both use it.
//!
//! - The audit-log reason is added here, percent-encoded, because twilight's own helper is
//!   private.
//! - Reads (GET) are retried on 5xx and network errors like every other read; writes never are.
//! - [`redact`] removes `token` fields and webhook URLs from response JSON.

use guildsmith_app::ports::{PortError, PortResult};
use http::header::{HeaderName, HeaderValue};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use twilight_http::Client;
use twilight_http::request::{Form, Method, RequestBuilder};
use twilight_http::response::marker::EmptyBody;

use crate::error::map_err;
use crate::retry;

/// Discord's limit on `X-Audit-Log-Reason`, in characters.
pub(crate) const AUDIT_MAX: usize = 512;
const PREFIX: &str = "guildsmith: ";

/// Characters left as-is in query values: RFC 3986 unreserved.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// The audit reason as Discord wants it: `guildsmith: …`, at most 512 characters, then
/// percent-encoded UTF-8.
pub(crate) fn audit_value(reason: &str) -> String {
    let mut text = if reason.starts_with(PREFIX) { reason.to_owned() } else { format!("{PREFIX}{reason}") };
    if text.chars().count() > AUDIT_MAX {
        text = text.chars().take(AUDIT_MAX - 1).collect::<String>() + "…";
    }
    utf8_percent_encode(&text, NON_ALPHANUMERIC).to_string()
}

/// `X-Audit-Log-Reason` for `reason`.
pub(crate) fn audit(reason: &str) -> PortResult<impl Iterator<Item = (HeaderName, HeaderValue)>> {
    let value = HeaderValue::from_str(&audit_value(reason)).map_err(|e| PortError::Discord {
        status: 0,
        code: None,
        message: format!("bad audit reason: {e}"),
    })?;
    Ok(std::iter::once((HeaderName::from_static("x-audit-log-reason"), value)))
}

/// `path?k=v&…`, with the values percent-encoded.
pub(crate) fn path_and_query(path: &str, query: &[(String, String)]) -> String {
    let path = path.trim_start_matches('/');
    if query.is_empty() {
        return path.to_owned();
    }
    let q: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", utf8_percent_encode(k, QUERY), utf8_percent_encode(v, QUERY)))
        .collect();
    format!("{path}?{}", q.join("&"))
}

/// A response: its status and body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    /// The body as JSON (`null` when empty).
    pub fn json(&self) -> PortResult<Value> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body).map_err(|e| PortError::Discord {
            status: self.status,
            code: None,
            message: format!("couldn't read Discord's response: {e}"),
        })
    }
}

/// A request body: JSON bytes, or a multipart form (JSON plus files).
enum Body<'a> {
    None,
    Json(&'a [u8]),
    Form(Form),
}

async fn send_once(
    http: &Client,
    method: Method,
    target: &str,
    body: Body<'_>,
    reason: Option<&str>,
) -> PortResult<Response> {
    send_built(http, RequestBuilder::raw(method, target.to_owned()), body, reason).await
}

async fn send_built(
    http: &Client,
    mut builder: RequestBuilder,
    body: Body<'_>,
    reason: Option<&str>,
) -> PortResult<Response> {
    match body {
        Body::None => {}
        Body::Json(b) => builder = builder.body(b.to_vec()),
        Body::Form(f) => builder = builder.form(f),
    }
    if let Some(r) = reason {
        builder = builder.headers(audit(r)?);
    }
    let req = builder.build().map_err(map_err)?;
    let res = http.request::<EmptyBody>(req).await.map_err(map_err)?;
    let status = res.status().get();
    let body = res.bytes().await.map_err(|e| PortError::Network(format!("reading Discord's response: {e}")))?;
    Ok(Response { status, body })
}

/// Sends one request. `reason` is required for writes by the callers' policy, not here.
pub(crate) async fn send(
    http: &Client,
    method: Method,
    path: &str,
    query: &[(String, String)],
    body: Option<&Value>,
    reason: Option<&str>,
) -> PortResult<Response> {
    let target = path_and_query(path, query);
    let bytes = body.map(serde_json::to_vec).transpose().map_err(|e| PortError::Discord {
        status: 0,
        code: None,
        message: format!("bad request body: {e}"),
    })?;
    let body = || bytes.as_deref().map_or(Body::None, Body::Json);
    if method == Method::Get {
        retry::read(|| send_once(http, method, &target, body(), reason)).await
    } else {
        send_once(http, method, &target, body(), reason).await
    }
}

/// Sends JSON plus files as `multipart/form-data`: the JSON as `payload_json` (with an
/// `attachments` list naming each file), each file as `files[n]`. Never retried.
pub(crate) async fn send_files(
    http: &Client,
    method: Method,
    path: &str,
    payload: &Value,
    files: &[(String, Vec<u8>)],
    reason: Option<&str>,
) -> PortResult<Response> {
    let mut payload = payload.clone();
    if let Value::Object(map) = &mut payload {
        let list: Vec<Value> =
            files.iter().enumerate().map(|(i, (name, _))| serde_json::json!({"id": i, "filename": name})).collect();
        map.insert("attachments".to_owned(), Value::Array(list));
    }
    let json = serde_json::to_vec(&payload).map_err(|e| PortError::Discord {
        status: 0,
        code: None,
        message: format!("bad request body: {e}"),
    })?;
    let mut form = Form::new().json_part(b"payload_json", &json);
    for (i, (name, bytes)) in files.iter().enumerate() {
        form = form.file_part(format!("files[{i}]").as_bytes(), name.as_bytes(), bytes);
    }
    send_once(http, method, &path_and_query(path, &[]), Body::Form(form), reason).await
}

/// Sends a ready multipart form (e.g. a sticker upload, whose fields aren't `payload_json`).
/// Never retried.
pub(crate) async fn send_form(
    http: &Client,
    method: Method,
    path: &str,
    form: Form,
    reason: Option<&str>,
) -> PortResult<Response> {
    send_once(http, method, &path_and_query(path, &[]), Body::Form(form), reason).await
}

/// Posts to a webhook's token route without the bot token (`webhooks/{id}/{token}…`). Errors
/// never include the path, which holds the webhook's token.
pub(crate) async fn send_webhook(
    http: &Client,
    path_and_query: String,
    payload: &Value,
    files: &[(String, Vec<u8>)],
) -> PortResult<Response> {
    let builder = RequestBuilder::raw(Method::Post, path_and_query).use_authorization_token(false);
    let res = if files.is_empty() {
        let json = serde_json::to_vec(payload).map_err(|e| PortError::Discord {
            status: 0,
            code: None,
            message: format!("bad request body: {e}"),
        })?;
        send_built(http, builder, Body::Json(&json), None).await
    } else {
        let mut payload = payload.clone();
        if let Value::Object(map) = &mut payload {
            let list: Vec<Value> =
                files.iter().enumerate().map(|(i, (name, _))| serde_json::json!({"id": i, "filename": name})).collect();
            map.insert("attachments".to_owned(), Value::Array(list));
        }
        let json = serde_json::to_vec(&payload).map_err(|e| PortError::Discord {
            status: 0,
            code: None,
            message: format!("bad request body: {e}"),
        })?;
        let mut form = Form::new().json_part(b"payload_json", &json);
        for (i, (name, bytes)) in files.iter().enumerate() {
            form = form.file_part(format!("files[{i}]").as_bytes(), name.as_bytes(), bytes);
        }
        send_built(http, builder, Body::Form(form), None).await
    };
    res.map_err(|e| match e {
        PortError::Discord { status, code, .. } => {
            PortError::Discord { status, code, message: "the webhook rejected the message".to_owned() }
        }
        PortError::Network(_) => PortError::Network("posting through the webhook failed".to_owned()),
        other => other,
    })
}

/// A JSON response, parsed into `R`.
pub(crate) fn parse<R: DeserializeOwned>(res: &Response, what: &str) -> PortResult<R> {
    serde_json::from_value(res.json()?).map_err(|e| PortError::Discord {
        status: res.status,
        code: None,
        message: format!("unexpected response from {what}: {e}"),
    })
}

/// Sends a typed body and reads a typed response (for `ext` endpoints).
pub(crate) async fn json<B: Serialize, R: DeserializeOwned>(
    http: &Client,
    method: Method,
    path: &str,
    query: &[(String, String)],
    body: Option<&B>,
    reason: Option<&str>,
) -> PortResult<R> {
    let body = body.map(serde_json::to_value).transpose().map_err(|e| PortError::Discord {
        status: 0,
        code: None,
        message: format!("bad request body: {e}"),
    })?;
    let res = send(http, method, path, query, body.as_ref(), reason).await?;
    let v = res.json()?;
    serde_json::from_value(v).map_err(|e| PortError::Discord {
        status: res.status,
        code: None,
        message: format!("unexpected response from {path}: {e}"),
    })
}

pub use guildsmith_core::raw::{is_webhook_url, redact};
