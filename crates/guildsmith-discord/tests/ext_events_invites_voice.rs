//! T045: the events, invites and voice ext endpoints against a mock Discord: event bodies with
//! `recurrence_rule`, invite `role_ids` and target users (bulk, at most 1000), and
//! `PUT channels/{c}/voice-status`.

use guildsmith_app::ports::{EventFields, EventLocation, EventsPort, InvitesPort, NewInvite, VoicePort};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
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
    serde_json::from_slice(&r.expect("sent").body).unwrap()
}

fn event_json(rule: &Value) -> Value {
    json!({"id": "700", "guild_id": "1000", "channel_id": "103", "creator_id": "2", "name": "Game night",
        "description": null, "scheduled_start_time": "2026-10-24T03:00:00+00:00", "scheduled_end_time": null,
        "privacy_level": 2, "status": 1, "entity_type": 2, "entity_id": null, "entity_metadata": null,
        "user_count": 4, "recurrence_rule": rule})
}

#[tokio::test]
async fn events_carry_the_recurrence_rule() {
    let s = server().await;
    let rule = json!({"start": "2026-10-24T03:00:00Z", "frequency": 2, "interval": 1, "by_weekday": [4]});
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/scheduled-events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(event_json(&rule)))
        .mount(&s)
        .await;
    let f = EventFields {
        name: Some("Game night".to_owned()),
        start: Some("2026-10-24T03:00:00Z".to_owned()),
        location: Some(EventLocation::Voice(Id(103))),
        recurrence: Some(rule.clone()),
        ..EventFields::default()
    };
    let e = client(&s).create_event(Id(1000), &f, "guildsmith: schedule").await.unwrap();
    assert_eq!((e.id, e.interested, e.location.clone()), (Id(700), 4, EventLocation::Voice(Id(103))));
    let sent = body(&s, "POST", "/api/v10/guilds/1000/scheduled-events").await;
    assert_eq!(sent["recurrence_rule"], rule);
    assert_eq!(
        (sent["entity_type"].clone(), sent["channel_id"].clone(), sent["privacy_level"].clone()),
        (json!(2), json!("103"), json!(2))
    );
    // Recurrence limits are the service's job; the body sends no count or end.
    assert!(sent["recurrence_rule"].get("count").is_none() && sent["recurrence_rule"].get("end").is_none());
}

#[tokio::test]
async fn external_updates_clear_the_channel() {
    let s = server().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000/scheduled-events/700"))
        .respond_with(ResponseTemplate::new(200).set_body_json(event_json(&Value::Null)))
        .mount(&s)
        .await;
    let f = EventFields {
        location: Some(EventLocation::External("Park".to_owned())),
        end: Some("2026-10-24T05:00:00Z".to_owned()),
        ..EventFields::default()
    };
    client(&s).update_event(Id(1000), Id(700), &f, "r").await.unwrap();
    let sent = body(&s, "PATCH", "/api/v10/guilds/1000/scheduled-events/700").await;
    assert_eq!(
        sent,
        json!({"scheduled_end_time": "2026-10-24T05:00:00Z", "entity_type": 3,
        "entity_metadata": {"location": "Park"}, "channel_id": null})
    );
}

#[tokio::test]
async fn invites_send_role_ids_and_target_users() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/invites"))
        .and(body_json(json!({"max_age": 604_800, "max_uses": 10, "temporary": false, "unique": true, "role_ids": ["11"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code": "abc", "channel": {"id": "101", "name": "general"},
            "max_age": 604_800, "max_uses": 10, "uses": 0, "temporary": false, "created_at": "2026-10-19T12:00:00+00:00",
            "role_ids": ["11"]})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/invites/abc/target-users"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/invites/abc/target-users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": "51", "username": "sam"}])))
        .mount(&s)
        .await;
    let c = client(&s);
    let inv = NewInvite { max_age: 604_800, max_uses: 10, temporary: false, unique: true, roles: vec![Id(11)] };
    let info = c.create_invite(Id(101), &inv, "r").await.unwrap();
    assert_eq!((info.code.as_str(), info.roles.clone()), ("abc", vec![Id(11)]));
    let users: Vec<Id> = (0..1000).map(Id).collect();
    c.add_invite_target_users("abc", &users).await.unwrap();
    let sent = body(&s, "POST", "/api/v10/invites/abc/target-users").await;
    assert_eq!(sent["user_ids"].as_array().unwrap().len(), 1000);
    assert_eq!(c.invite_target_users("abc").await.unwrap(), [Id(51)]);
}

#[tokio::test]
async fn voice_status_and_voice_states() {
    let s = server().await;
    Mock::given(method("PUT"))
        .and(path("/api/v10/channels/103/voice-status"))
        .and(body_json(json!({"status": "Playing chess"})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/voice-states/51"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"code": 10065, "message": "Unknown Voice State"})),
        )
        .mount(&s)
        .await;
    let c = client(&s);
    c.set_voice_status(Id(103), Some("Playing chess"), "r").await.unwrap();
    assert_eq!(c.voice_state(Id(1000), Id(51)).await.unwrap(), None, "not in voice");
}
