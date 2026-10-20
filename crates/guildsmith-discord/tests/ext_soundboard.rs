//! T068: soundboard sounds against a mock Discord: `guilds/{g}/soundboard-sounds[/{id}]`, the
//! list as `{items}`, and sounds sent as MP3/Ogg data URIs.

use guildsmith_app::ports::{ExpressionEdit, ExpressionKind, ExpressionsPort, NewExpression};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_core::asset::FileType;
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

fn sound(id: &str, name: &str) -> Value {
    json!({"name": name, "sound_id": id, "volume": 0.5, "emoji_id": null, "emoji_name": "🔔", "guild_id": "1000",
        "available": true, "user": {"id": "2", "username": "guildsmith"}})
}

async fn body(s: &MockServer, verb: &str) -> Value {
    let r = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method.as_str() == verb && r.url.path().contains("soundboard"));
    serde_json::from_slice(&r.expect("sent").body).unwrap()
}

#[tokio::test]
async fn sounds_list_create_update_and_delete() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/soundboard-sounds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [sound("40", "ding")]})))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/soundboard-sounds"))
        .respond_with(ResponseTemplate::new(201).set_body_json(sound("41", "airhorn")))
        .mount(&s)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000/soundboard-sounds/41"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sound("41", "horn")))
        .mount(&s)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v10/guilds/1000/soundboard-sounds/41"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    let c = client(&s);
    let list = c.expressions(Id(1000), ExpressionKind::Sound).await.unwrap();
    assert_eq!((list[0].id, list[0].name.as_str(), list[0].emoji.as_deref()), (Id(40), "ding", Some("🔔")));
    assert_eq!(list[0].url, "https://cdn.discordapp.com/soundboard-sounds/40");

    let new = NewExpression {
        kind: ExpressionKind::Sound,
        name: "airhorn".to_owned(),
        bytes: b"ID3\x04fake mp3".to_vec(),
        file_type: FileType::Mp3,
        roles: Vec::new(),
        description: String::new(),
        tags: String::new(),
        volume: Some(0.5),
        emoji: Some("📯".to_owned()),
    };
    let created = c.create_expression(Id(1000), &new, "r").await.unwrap();
    assert_eq!(created.id, Id(41));
    let sent = body(&s, "POST").await;
    assert!(sent["sound"].as_str().unwrap().starts_with("data:audio/mpeg;base64,"), "{sent}");
    assert_eq!(
        (sent["volume"].clone(), sent["emoji_name"].clone(), sent["emoji_id"].clone()),
        (json!(0.5), json!("📯"), Value::Null)
    );

    let edit = ExpressionEdit { name: Some("horn".to_owned()), ..ExpressionEdit::default() };
    c.update_expression(Id(1000), ExpressionKind::Sound, Id(41), &edit, "r").await.unwrap();
    assert_eq!(body(&s, "PATCH").await, json!({"name": "horn"}), "unset fields are left out");
    c.delete_expression(Id(1000), ExpressionKind::Sound, Id(41), "r").await.unwrap();
}

#[tokio::test]
async fn stickers_upload_as_multipart_fields() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/stickers"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": "60", "name": "wave", "description": "hi",
            "tags": "👋", "type": 2, "format_type": 1, "available": true, "guild_id": "1000"})))
        .mount(&s)
        .await;
    let new = NewExpression {
        kind: ExpressionKind::Sticker,
        name: "wave".to_owned(),
        bytes: vec![0x89, b'P', b'N', b'G'],
        file_type: FileType::Png,
        roles: Vec::new(),
        description: "hi".to_owned(),
        tags: "👋".to_owned(),
        volume: None,
        emoji: None,
    };
    let created = client(&s).create_expression(Id(1000), &new, "r").await.unwrap();
    assert_eq!((created.id, created.tags.as_deref()), (Id(60), Some("👋")));
    let req = s.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("/stickers")).unwrap();
    let text = String::from_utf8_lossy(&req.body);
    for part in ["name=\"name\"", "name=\"description\"", "name=\"tags\"", "name=\"file\"; filename=\"sticker.png\""] {
        assert!(text.contains(part), "{part} missing: {text}");
    }
}
