//! The settings adapter against a mock Discord: the guild PATCH body carries only what changes
//! (with `null` to clear), including `description` and `safety_alerts_channel_id`; onboarding
//! is PUT whole; templates are read by code.

use guildsmith_app::ports::{Onboarding, OnboardingOption, OnboardingPrompt, SettingsChange, SettingsPort};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
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

async fn body(s: &MockServer, verb: &str, p: &str) -> Value {
    let r = s.received_requests().await.unwrap().into_iter().find(|r| r.method.as_str() == verb && r.url.path() == p);
    serde_json::from_slice(&r.expect("sent").body).unwrap()
}

#[tokio::test]
async fn settings_patch_sends_only_changes() {
    let s = server().await;
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "1000", "name": "Lab", "description": "A lab",
            "afk_timeout": 900, "features": ["COMMUNITY"], "safety_alerts_channel_id": "101"})),
        )
        .mount(&s)
        .await;
    let change = SettingsChange {
        description: Some(Some("A lab".to_owned())),
        safety_alerts_channel_id: Some(Some(Id(101))),
        afk_channel_id: Some(None),
        afk_timeout: Some(900),
        ..SettingsChange::default()
    };
    let got = client(&s).update_guild_settings(Id(1000), &change, "guildsmith: update settings").await.unwrap();
    assert_eq!(got.description.as_deref(), Some("A lab"));
    assert_eq!(
        body(&s, "PATCH", "/api/v10/guilds/1000").await,
        json!({"description": "A lab", "safety_alerts_channel_id": "101", "afk_channel_id": null, "afk_timeout": 900})
    );
}

#[tokio::test]
async fn onboarding_is_put_whole_and_templates_read_by_code() {
    let s = server().await;
    let reply = json!({"guild_id": "1000", "enabled": true, "mode": 1, "default_channel_ids": ["101"],
        "prompts": [{"id": "5", "title": "Games?", "type": 0, "single_select": false, "required": false, "in_onboarding": true,
            "options": [{"id": "6", "title": "Minecraft", "description": null, "emoji": {"id": null, "name": "⛏️"},
                "role_ids": ["11"], "channel_ids": []}]}]});
    Mock::given(method("PUT"))
        .and(path("/api/v10/guilds/1000/onboarding"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/templates/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"code": "abc", "name": "Gaming", "usage_count": 3,
            "created_at": "2020-01-01T00:00:00+00:00", "updated_at": "2020-01-01T00:00:00+00:00",
            "serialized_source_guild": {"name": "Gaming", "roles": [], "channels": []}, "is_dirty": null}),
        ))
        .mount(&s)
        .await;
    let o = Onboarding {
        enabled: true,
        mode: 1,
        default_channel_ids: vec![Id(101)],
        prompts: vec![OnboardingPrompt {
            id: Id(5),
            title: "Games?".to_owned(),
            kind: 0,
            single_select: false,
            required: false,
            in_onboarding: true,
            options: vec![OnboardingOption {
                id: Id(6),
                title: "Minecraft".to_owned(),
                description: None,
                emoji: Some("⛏️".to_owned()),
                role_ids: vec![Id(11)],
                channel_ids: Vec::new(),
            }],
        }],
    };
    let c = client(&s);
    let stored = c.set_onboarding(Id(1000), &o, "r").await.unwrap();
    assert_eq!(stored, o);
    let sent = body(&s, "PUT", "/api/v10/guilds/1000/onboarding").await;
    assert_eq!(sent["prompts"][0]["options"][0]["emoji_name"], "⛏️");
    assert_eq!(sent["mode"], 1);
    let t = c.get_template("abc").await.unwrap();
    assert_eq!((t.name.as_str(), t.dirty), ("Gaming", false));
}
