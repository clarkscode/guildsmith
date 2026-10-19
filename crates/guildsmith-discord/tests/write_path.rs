//! The adapter's write path against a mock Discord (wiremock): request shapes, audit-log
//! reasons, and no retries for writes (FR-013, research R7/R8).

use guildsmith_app::ports::{ChannelFields, ChannelPosition, DiscordPort, PortError, RoleFields};
use guildsmith_app::secret::Secret;
use guildsmith_core::snapshot::{LiveChannelKind, LiveOverwrite, OverwriteKind};
use guildsmith_core::spec::{ChannelKind, ForumTag};
use guildsmith_core::{Id, Permission, PermissionSet};
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REASON: &str = "guildsmith: create text channel #staff-room in Info";
/// `REASON`, percent-encoded as Discord requires.
const REASON_ENCODED: &str = "guildsmith%3A%20create%20text%20channel%20%23staff%2Droom%20in%20Info";

fn fixture(name: &str) -> Value {
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn role_json(id: &str) -> Value {
    json!({"id": id, "name": "Staff", "color": 0, "colors": {"primary_color": 0, "secondary_color": null,
        "tertiary_color": null}, "hoist": false, "icon": null, "unicode_emoji": null, "position": 1,
        "permissions": "0", "managed": false, "mentionable": false, "flags": 0})
}

fn channel_json(id: &str) -> Value {
    json!({"id": id, "type": 0, "guild_id": "1000", "name": "staff-room", "position": 1, "permission_overwrites": [],
        "nsfw": false, "flags": 0})
}

async fn server() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .mount(&s)
        .await;
    s
}

fn client(s: &MockServer) -> TwilightDiscord {
    TwilightDiscord::with_proxy(Some(Secret::new("test-token")), s.address().to_string())
}

async fn bodies(s: &MockServer, verb: &str, p: &str) -> Vec<Value> {
    s.received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == verb && r.url.path() == p)
        .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
        .collect()
}

#[tokio::test]
async fn create_channel_sends_fields_parent_and_overwrites_in_one_request() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/channels"))
        .and(header("x-audit-log-reason", REASON_ENCODED))
        .respond_with(ResponseTemplate::new(201).set_body_json(channel_json("555")))
        .expect(1)
        .mount(&s)
        .await;
    let view: PermissionSet = [Permission::ViewChannel].into_iter().collect();
    let fields = ChannelFields {
        name: Some("staff-room".to_owned()),
        kind: Some(LiveChannelKind::Known(ChannelKind::Text)),
        parent_id: Some(Some(Id(100))),
        topic: Some("staff only".to_owned()),
        overwrites: Some(vec![
            LiveOverwrite { id: Id(1000), kind: OverwriteKind::Role, allow: PermissionSet::EMPTY, deny: view },
            LiveOverwrite { id: Id(50), kind: OverwriteKind::Member, allow: view, deny: PermissionSet::EMPTY },
        ]),
        ..ChannelFields::default()
    };
    let id = client(&s).create_channel(Id(1000), &fields, REASON).await.unwrap();
    assert_eq!(id, Id(555));
    let body = &bodies(&s, "POST", "/api/v10/guilds/1000/channels").await[0];
    assert_eq!(
        body,
        &json!({
            "name": "staff-room", "type": 0, "parent_id": "100", "topic": "staff only",
            "permission_overwrites": [
                {"id": "1000", "type": 0, "allow": "0", "deny": "1024"},
                {"id": "50", "type": 1, "allow": "1024", "deny": "0"}
            ]
        })
    );
}

#[tokio::test]
async fn update_channel_sends_nulls_to_clear_and_tags_without_ids() {
    let s = server().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/channels/106"))
        .respond_with(ResponseTemplate::new(200).set_body_json(channel_json("106")))
        .mount(&s)
        .await;
    let fields = ChannelFields {
        parent_id: Some(None),
        topic: Some(String::new()),
        rtc_region: Some(None),
        default_reaction: Some(Some("👍".to_owned())),
        tags: Some(vec![
            ForumTag {
                id: Some(Id(201)),
                name: "question".to_owned(),
                emoji: Some("❓".to_owned()),
                moderated: Some(false),
            },
            ForumTag { id: None, name: "idea".to_owned(), emoji: Some("777".to_owned()), moderated: None },
        ]),
        ..ChannelFields::default()
    };
    client(&s).update_channel(Id(1000), Id(106), &fields, "guildsmith: update channel #help").await.unwrap();
    let body = &bodies(&s, "PATCH", "/api/v10/channels/106").await[0];
    assert_eq!(
        body,
        &json!({
            "parent_id": null, "topic": null, "rtc_region": null,
            "default_reaction_emoji": {"emoji_name": "👍"},
            "available_tags": [
                {"id": "201", "name": "question", "moderated": false, "emoji_name": "❓"},
                {"name": "idea", "moderated": false, "emoji_id": "777"}
            ]
        })
    );
}

#[tokio::test]
async fn channel_reorders_send_positions_only() {
    let s = server().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000/channels"))
        .and(header("x-audit-log-reason", "guildsmith%3A%20reorder%20channels%20%282%29"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    let positions = [
        ChannelPosition { id: Id(101), position: 2, parent_id: None },
        ChannelPosition { id: Id(102), position: 1, parent_id: None },
    ];
    client(&s).reorder_channels(Id(1000), &positions, "guildsmith: reorder channels (2)").await.unwrap();
    let body = &bodies(&s, "PATCH", "/api/v10/guilds/1000/channels").await[0];
    assert_eq!(body, &json!([{"id": "101", "position": 2}, {"id": "102", "position": 1}]));
}

#[tokio::test]
async fn role_writes_carry_the_audit_reason() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/roles"))
        .and(header("x-audit-log-reason", "guildsmith%3A%20create%20role%20%40Staff"))
        .and(body_partial_json(json!({"name": "Staff", "hoist": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(role_json("77")))
        .expect(1)
        .mount(&s)
        .await;
    let fields = RoleFields { name: Some("Staff".to_owned()), hoist: Some(true), ..RoleFields::default() };
    assert_eq!(client(&s).create_role(Id(1000), &fields, "guildsmith: create role @Staff").await.unwrap(), Id(77));
}

#[tokio::test]
async fn writes_are_never_retried() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/channels"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"message": "oops", "code": 0})))
        .mount(&s)
        .await;
    let fields = ChannelFields { name: Some("x".to_owned()), ..ChannelFields::default() };
    let err = client(&s).create_channel(Id(1000), &fields, REASON).await.unwrap_err();
    assert!(matches!(err, PortError::Discord { status: 500, .. }), "{err:?}");
    assert_eq!(bodies(&s, "POST", "/api/v10/guilds/1000/channels").await.len(), 1, "one attempt only");
}
