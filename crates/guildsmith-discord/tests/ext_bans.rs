//! T022: the moderation ext endpoints against a mock Discord: bulk ban
//! (`POST guilds/{g}/bulk-ban`), prune with `compute_prune_count = false`, the audit log and
//! incident actions.

use guildsmith_app::ports::{Incidents, ModerationPort, PortError};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
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

async fn body(s: &MockServer, verb: &str, p: &str) -> Value {
    let r = s.received_requests().await.unwrap().into_iter().find(|r| r.method.as_str() == verb && r.url.path() == p);
    serde_json::from_slice(&r.expect("request sent").body).unwrap_or(Value::Null)
}

#[tokio::test]
async fn bulk_ban_sends_ids_and_seconds_and_reads_banned_and_failed() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/bulk-ban"))
        .and(body_json(json!({"user_ids": ["50", "51", "52"], "delete_message_seconds": 604_800})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"banned_users": ["50", "51"], "failed_users": ["52"]})),
        )
        .expect(1)
        .mount(&s)
        .await;
    let res = client(&s).bulk_ban(Id(1000), &[Id(50), Id(51), Id(52)], 604_800, "guildsmith: ban 3").await.unwrap();
    assert_eq!(res.banned, [Id(50), Id(51)]);
    assert_eq!(res.failed, [Id(52)]);
    let req = s.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("bulk-ban")).unwrap();
    assert!(req.headers.get("x-audit-log-reason").is_some());
}

#[tokio::test]
async fn bulk_ban_without_deletion_leaves_the_field_out_and_maps_500000() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/bulk-ban"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({"code": 500_000, "message": "Failed to ban users"})),
        )
        .mount(&s)
        .await;
    let err = client(&s).bulk_ban(Id(1000), &[Id(50)], 0, "r").await.unwrap_err();
    assert!(matches!(err, PortError::Discord { code: Some(500_000), .. }), "{err:?}");
    assert_eq!(body(&s, "POST", "/api/v10/guilds/1000/bulk-ban").await, json!({"user_ids": ["50"]}));
}

#[tokio::test]
async fn prune_count_and_prune_without_a_count() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/prune"))
        .and(query_param("days", "30"))
        .and(query_param("include_roles", "11,12"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"pruned": 7})))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/prune"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"pruned": null})))
        .mount(&s)
        .await;
    let c = client(&s);
    assert_eq!(c.prune_count(Id(1000), 30, &[Id(11), Id(12)]).await.unwrap(), 7);
    assert_eq!(c.prune(Id(1000), 30, &[], false, "r").await.unwrap(), None);
    assert_eq!(body(&s, "POST", "/api/v10/guilds/1000/prune").await, json!({"days": 30, "compute_prune_count": false}));
}

#[tokio::test]
async fn the_audit_log_keeps_only_what_guildsmith_reports() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/audit-logs"))
        .and(query_param("action_type", "22"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "audit_log_entries": [{"id": "900", "action_type": 22, "user_id": "50", "target_id": "51",
                "reason": "spam", "changes": [{"key": "nick", "old_value": "a", "new_value": "b"}]}],
            "users": [], "webhooks": [], "integrations": [], "threads": [], "application_commands": [],
            "auto_moderation_rules": [], "guild_scheduled_events": []
        })))
        .mount(&s)
        .await;
    let entries = client(&s).audit_log(Id(1000), Some(22), None, None, 10).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].id, entries[0].user_id, entries[0].target_id), (Id(900), Some(Id(50)), Some(Id(51))));
    assert_eq!(entries[0].changes, ["nick"]);
}

#[tokio::test]
async fn incident_actions_send_both_times() {
    let s = server().await;
    Mock::given(method("PUT"))
        .and(path("/api/v10/guilds/1000/incident-actions"))
        .and(body_json(json!({"invites_disabled_until": "2026-10-20T12:00:00Z", "dms_disabled_until": null})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "invites_disabled_until": "2026-10-20T12:00:00Z", "dms_disabled_until": null
        })))
        .expect(1)
        .mount(&s)
        .await;
    let want = Incidents { invites_disabled_until: Some("2026-10-20T12:00:00Z".to_owned()), dms_disabled_until: None };
    assert_eq!(client(&s).set_incidents(Id(1000), &want, "r").await.unwrap(), want);
}
