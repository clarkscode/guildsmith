//! T105: the raw service against `FakeDiscord`: GETs run at once, writes preview until
//! confirmed, read-only and the allowlist apply (after looking up a channel's, webhook's or
//! invite's server), secrets in bodies are refused, and long responses are cut.

use std::collections::BTreeSet;
use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_app::ports::WebhookKind;
use guildsmith_app::services::inspect;
use guildsmith_app::services::invites::{self, InviteInput};
use guildsmith_app::services::raw::{self, RawResult};
use guildsmith_core::Id;
use guildsmith_core::raw::RawMethod;
use serde_json::json;

fn app_with(policy: Policy) -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), policy))
}

fn app() -> Arc<App<FakeDiscord>> {
    app_with(Policy::default())
}

fn refusal(e: &ServiceError) -> Option<RefusalKind> {
    match e {
        ServiceError::Refused(r) => Some(r.kind),
        _ => None,
    }
}

fn done(r: RawResult) -> raw::RawResponse {
    match r {
        RawResult::Done(d) => d,
        RawResult::Preview(p) => panic!("expected a response, got {p:?}"),
    }
}

#[tokio::test]
async fn get_runs_without_confirm() {
    let app = app();
    app.port.set_raw_response(RawMethod::Get, "guilds/1000/audit-logs", 200, json!({"audit_log_entries": []}));
    let r = done(
        raw::discord_request(&app, None, "get", "/api/v10/guilds/1000/audit-logs?limit=5", &[], None, false, None)
            .await
            .unwrap(),
    );
    assert_eq!(r.status, 200);
    assert!(r.body.contains("audit_log_entries"), "{}", r.body);
    assert!(!r.truncated);
    let sent = app.port.raw_requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].path, "guilds/1000/audit-logs");
    assert_eq!(sent[0].query, [("limit".to_owned(), "5".to_owned())]);
    assert_eq!(app.port.writes(), [], "a GET is no write");
}

#[tokio::test]
async fn guild_placeholder_is_resolved() {
    let app = app();
    app.port.set_raw_response(RawMethod::Get, "guilds/1000/roles", 200, json!([]));
    raw::discord_request(&app, Some("1000"), "GET", "guilds/{guild}/roles", &[], None, false, None).await.unwrap();
    // With one server, it's found without being named.
    raw::discord_request(&app, None, "GET", "guilds/{guild}/roles", &[], None, false, None).await.unwrap();
    assert!(app.port.raw_requests().iter().all(|r| r.path == "guilds/1000/roles"));
}

#[tokio::test]
async fn writes_preview_method_path_query_and_body() {
    let app = app();
    let body = json!({"name": "renamed"});
    let q = [("with_counts".to_owned(), "true".to_owned())];
    let r = raw::discord_request(&app, None, "PATCH", "guilds/1000", &q, Some(&body), false, None).await.unwrap();
    let RawResult::Preview(p) = r else { panic!("expected a preview, got {r:?}") };
    assert!(p.preview);
    assert_eq!(p.guild, Some(fixtures::GUILD));
    assert_eq!(p.would.method, RawMethod::Patch);
    assert_eq!(p.would.path, "guilds/1000");
    assert_eq!(p.would.query, q);
    assert_eq!(p.would.body, Some(body));
    let shown = serde_json::to_value(&p).unwrap();
    for k in ["method", "path", "query", "body"] {
        assert!(shown["would"].get(k).is_some(), "{k} missing from {shown}");
    }
    assert_eq!(app.port.raw_requests(), [], "nothing sent");
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn read_only_refuses_writes_but_not_reads() {
    let app = app_with(Policy { read_only: true, ..Policy::default() });
    for confirm in [false, true] {
        let e = raw::discord_request(&app, None, "DELETE", "guilds/1000/roles/11", &[], None, confirm, None)
            .await
            .unwrap_err();
        assert_eq!(refusal(&e), Some(RefusalKind::ReadOnly), "{e}");
    }
    assert_eq!(app.port.raw_requests(), []);
    app.port.set_raw_response(RawMethod::Get, "guilds/1000/roles", 200, json!([]));
    assert!(raw::discord_request(&app, None, "GET", "guilds/1000/roles", &[], None, false, None).await.is_ok());
}

#[tokio::test]
async fn servers_outside_the_allowlist_are_refused_after_lookup() {
    let app = app_with(Policy { allowlist: Some(BTreeSet::from([Id(5)])), ..Policy::default() });
    let hook = app.port.seed_webhook(fixtures::GUILD, fixtures::GENERAL, "CI", WebhookKind::Incoming, Some("tok"));
    // Invites are created with the allowlist off, then checked with it on.
    let open = app_with(Policy::default());
    let code =
        invites::invite_create(&open, Some("1000"), "#general", &InviteInput::default(), None).await.unwrap().code;
    let invite_app = Arc::new(App::new(
        Arc::try_unwrap(open).ok().expect("one owner").port,
        Policy { allowlist: Some(BTreeSet::from([Id(5)])), ..Policy::default() },
    ));
    // Confirmed deletes: if the allowlist were skipped they would be sent.
    let cases = [
        (&app, format!("guilds/{}/roles/11", fixtures::GUILD)),
        (&app, format!("channels/{}", fixtures::GENERAL)),
        (&app, format!("webhooks/{hook}")),
        (&invite_app, format!("invites/{code}")),
    ];
    for (app, path) in cases {
        let before = app.port.raw_requests().len();
        let writes = app.port.writes().len();
        let e = raw::discord_request(app, None, "DELETE", &path, &[], None, true, None).await.unwrap_err();
        assert_eq!(refusal(&e), Some(RefusalKind::NotAllowed), "{path}: {e}");
        let sent = &app.port.raw_requests()[before..];
        if path.starts_with("guilds/") {
            assert!(sent.is_empty(), "a guild path needs no lookup: {sent:?}");
        } else {
            assert_eq!(sent.len(), 1, "only the lookup was sent for {path}: {sent:?}");
            assert_eq!(sent[0].method, RawMethod::Get, "{path}: {sent:?}");
        }
        assert_eq!(app.port.writes().len(), writes, "{path}: nothing written");
    }
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn unknown_targets_and_refused_routes() {
    let app = app();
    let e =
        raw::discord_request(&app, None, "GET", "channels/987654/messages", &[], None, false, None).await.unwrap_err();
    assert_eq!(refusal(&e), Some(RefusalKind::Unsupported), "{e}");
    for path in ["oauth2/token", "gateway/bot", "applications/1/commands", "guilds"] {
        let e = raw::discord_request(&app, None, "POST", path, &[], Some(&json!({})), true, None).await.unwrap_err();
        assert_eq!(refusal(&e), Some(RefusalKind::Unsupported), "{path}: {e}");
    }
    assert!(matches!(
        raw::discord_request(&app, None, "BREW", "guilds/1000", &[], None, false, None).await,
        Err(ServiceError::Invalid(_))
    ));
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn bodies_with_secrets_are_refused() {
    let app = app();
    let token = format!("{}.{}.{}", "MTA0NTg2NzI5MzQ1NjE4OTQ0MA", "GhJkLm", "a".repeat(38));
    for body in
        [json!({"Authorization": "Bot x"}), json!({"nested": {"authorization": "y"}}), json!({"content": token})]
    {
        let e = raw::discord_request(&app, None, "POST", "channels/101/messages", &[], Some(&body), true, None)
            .await
            .unwrap_err();
        assert_eq!(refusal(&e), Some(RefusalKind::Unsupported), "{body}: {e}");
    }
    assert_eq!(app.port.raw_requests(), []);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn confirmed_writes_clear_the_cache_and_carry_the_reason() {
    let app = app();
    inspect::overview(&app, Some("1000")).await.unwrap();
    let reads = app.port.reads();
    inspect::overview(&app, Some("1000")).await.unwrap();
    assert_eq!(app.port.reads(), reads, "cached");
    app.port.set_raw_response(RawMethod::Patch, "guilds/1000", 200, json!({"id": "1000", "name": "renamed"}));
    let body = json!({"name": "renamed"});
    let r = done(
        raw::discord_request(&app, None, "PATCH", "guilds/1000", &[], Some(&body), true, Some("spring cleanup"))
            .await
            .unwrap(),
    );
    assert_eq!(r.status, 200);
    let w = app.port.writes();
    assert_eq!(w.len(), 1);
    let reason = w[0].reason.as_deref().unwrap();
    assert!(reason.contains("raw PATCH guilds/1000") && reason.contains("spring cleanup"), "{reason}");
    inspect::overview(&app, Some("1000")).await.unwrap();
    assert!(app.port.reads() > reads, "the cache was cleared");
}

#[tokio::test]
async fn responses_are_redacted_and_cut() {
    let app = app();
    app.port.set_raw_response(
        RawMethod::Get,
        "guilds/1000/webhooks",
        200,
        json!([{"id": "7", "token": "secret-webhook-token", "url": "https://discord.com/api/webhooks/7/secret-webhook-token"}]),
    );
    let r =
        done(raw::discord_request(&app, None, "GET", "guilds/1000/webhooks", &[], None, false, None).await.unwrap());
    assert!(!r.body.contains("secret-webhook-token"), "{}", r.body);

    let long = "x".repeat(60_000);
    app.port.set_raw_response(RawMethod::Get, "guilds/1000/members", 200, json!([{"bio": long}]));
    let r = done(raw::discord_request(&app, None, "GET", "guilds/1000/members", &[], None, false, None).await.unwrap());
    assert!(r.truncated);
    assert!(r.chars > 60_000);
    assert_eq!(r.body.chars().count(), raw::MAX_CHARS);
    assert!(r.body.ends_with("narrow it with query parameters]"), "{}", &r.body[r.body.len() - 100..]);
}
