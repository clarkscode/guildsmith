//! T008: the raw/ext request path against a mock Discord (wiremock): the audit header, token
//! and webhook-URL redaction, and retries (reads only, never 429s or writes).

use guildsmith_app::ports::RawPort;
use guildsmith_app::secret::Secret;
use guildsmith_core::raw::{RawMethod, RawRequest};
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server() -> MockServer {
    let s = MockServer::start().await;
    let me = std::fs::read_to_string(format!("{}/tests/fixtures/users_me.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(me, "application/json"))
        .mount(&s)
        .await;
    s
}

fn client(s: &MockServer) -> TwilightDiscord {
    TwilightDiscord::with_proxy(Some(Secret::new("test-token")), s.address().to_string())
}

fn req(m: RawMethod, p: &str, body: Option<Value>) -> RawRequest {
    RawRequest { method: m, path: p.to_owned(), query: Vec::new(), body }
}

async fn hits(s: &MockServer, verb: &str, p: &str) -> Vec<wiremock::Request> {
    s.received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == verb && r.url.path() == p)
        .collect()
}

#[tokio::test]
async fn audit_reason_is_prefixed_percent_encoded_and_cut_to_512_characters() {
    let s = server().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "1000"})))
        .mount(&s)
        .await;
    let c = client(&s);
    let r = req(RawMethod::Patch, "guilds/1000", Some(json!({"description": "x"})));
    c.raw_request(&r, Some("set the description: café ✓")).await.unwrap();
    let long = "é".repeat(700);
    c.raw_request(&r, Some(&format!("guildsmith: {long}"))).await.unwrap();

    let got = hits(&s, "PATCH", "/api/v10/guilds/1000").await;
    let header = |i: usize| got[i].headers.get("x-audit-log-reason").unwrap().to_str().unwrap().to_owned();
    let decode = |h: &str| percent_encoding::percent_decode_str(h).decode_utf8().unwrap().into_owned();

    let first = header(0);
    assert!(first.is_ascii() && !first.contains(' '), "percent-encoded: {first}");
    assert_eq!(decode(&first), "guildsmith: set the description: café ✓");

    let second = decode(&header(1));
    assert!(second.starts_with("guildsmith: "), "{second}");
    assert_eq!(second.chars().count(), 512, "cut to 512 characters");
    assert!(!second.starts_with("guildsmith: guildsmith: "), "prefix added only once");
}

#[tokio::test]
async fn reads_carry_no_audit_header() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/audit-logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"audit_log_entries": []})))
        .mount(&s)
        .await;
    client(&s).raw_request(&req(RawMethod::Get, "guilds/1000/audit-logs", None), Some("ignored")).await.unwrap();
    let got = hits(&s, "GET", "/api/v10/guilds/1000/audit-logs").await;
    assert!(got[0].headers.get("x-audit-log-reason").is_none());
}

#[tokio::test]
async fn tokens_are_removed_and_webhook_urls_redacted() {
    let s = server().await;
    let body = json!([
        {"id": "7", "type": 1, "name": "CI", "token": "wh-secret-token",
         "url": "https://discord.com/api/webhooks/7/wh-secret-token", "channel_id": "101"},
        {"id": "8", "type": 2, "name": "Follower", "source_channel": {"id": "1", "name": "news"}},
        {"note": "see https://canary.discord.com/api/v10/webhooks/9/abc for details"}
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&s)
        .await;
    let reply = client(&s).raw_request(&req(RawMethod::Get, "guilds/1000/webhooks", None), None).await.unwrap();
    let text = reply.json.to_string();
    assert!(!text.contains("wh-secret-token"), "{text}");
    assert!(!text.contains("/webhooks/9/abc"), "{text}");
    assert!(reply.json[0].get("token").is_none());
    assert_eq!(reply.json[0]["url"], "<redacted>");
    assert_eq!(reply.json[0]["name"], "CI");
    assert_eq!(reply.json[1]["source_channel"]["name"], "news");
}

#[tokio::test]
async fn a_429_is_retried_by_twilight_not_by_our_code() {
    let s = server().await;
    // Our code never retries a write, so a write that succeeds after a 429 was retried by
    // twilight (its limiter waits for a permit; here, behind a proxy, it re-sends at once).
    Mock::given(method("PUT"))
        .and(path("/api/v10/guilds/1000/bans/50"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(json!({"message": "You are being rate limited.", "retry_after": 0.0, "global": false})),
        )
        .up_to_n_times(1)
        .mount(&s)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v10/guilds/1000/bans/50"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&s)
        .await;
    let reply = client(&s).raw_request(&req(RawMethod::Put, "guilds/1000/bans/50", None), Some("ban")).await.unwrap();
    assert_eq!(reply.status, 204);
    assert_eq!(hits(&s, "PUT", "/api/v10/guilds/1000/bans/50").await.len(), 2);
}

#[tokio::test]
async fn writes_are_never_retried_on_5xx_but_reads_are() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/messages"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&s)
        .await;
    let err = client(&s)
        .raw_request(&req(RawMethod::Post, "channels/101/messages", Some(json!({"content": "hi"}))), Some("post"))
        .await
        .unwrap_err();
    assert!(err.is_retryable(), "a bodiless 502 is a server-side failure: {err:?}");
    assert_eq!(hits(&s, "POST", "/api/v10/channels/101/messages").await.len(), 1);

    Mock::given(method("GET"))
        .and(path("/api/v10/channels/101"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/channels/101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "101"})))
        .mount(&s)
        .await;
    let reply = client(&s).raw_request(&req(RawMethod::Get, "channels/101", None), None).await.unwrap();
    assert_eq!(reply.json["id"], "101");
    assert_eq!(hits(&s, "GET", "/api/v10/channels/101").await.len(), 3);
}

#[tokio::test]
async fn query_values_are_encoded() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/messages/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages": []})))
        .mount(&s)
        .await;
    let mut r = req(RawMethod::Get, "guilds/1000/messages/search", None);
    r.query = vec![("content".to_owned(), "hello world & more".to_owned()), ("limit".to_owned(), "25".to_owned())];
    client(&s).raw_request(&r, None).await.unwrap();
    let got = hits(&s, "GET", "/api/v10/guilds/1000/messages/search").await;
    let pairs: Vec<(String, String)> =
        got[0].url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    assert_eq!(pairs, r.query);
}
