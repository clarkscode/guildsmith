//! The webhooks adapter against a mock Discord: lists carry no tokens, create returns the
//! token separately, executing goes to the token route without the bot token, and errors from
//! that route never include the token.

use guildsmith_app::ports::{OutgoingMessage, WebhookKind, WebhooksPort};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_discord::TwilightDiscord;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "wh-route-token-0123456789";

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
    TwilightDiscord::with_proxy(Some(Secret::new("bot-token-abc")), s.address().to_string())
}

fn hook(id: &str, kind: u8) -> serde_json::Value {
    json!({"id": id, "type": kind, "guild_id": "1000", "channel_id": "101", "name": "CI", "avatar": null,
        "token": TOKEN, "url": format!("https://discord.com/api/webhooks/{id}/{TOKEN}"),
        "user": {"id": "1", "username": "owner"}, "application_id": null})
}

#[tokio::test]
async fn lists_drop_tokens_and_create_returns_it_separately() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([hook("700", 1), hook("701", 2)])))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hook("702", 1)))
        .mount(&s)
        .await;
    let c = client(&s);
    let list = c.guild_webhooks(Id(1000)).await.unwrap();
    assert_eq!(list.iter().map(|w| w.kind).collect::<Vec<_>>(), [WebhookKind::Incoming, WebhookKind::ChannelFollower]);
    assert!(!format!("{list:?}").contains(TOKEN));
    let (info, token) = c.create_webhook(Id(101), "CI", None, "guildsmith: create webhook CI").await.unwrap();
    assert_eq!(info.id, Id(702));
    assert_eq!(token.expose(), TOKEN);
    assert!(!format!("{info:?} {token:?}").contains(TOKEN));
}

#[tokio::test]
async fn execute_uses_the_token_route_without_the_bot_token_and_errors_hide_it() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v10/webhooks/700/{TOKEN}")))
        .and(query_param("wait", "true"))
        .and(query_param("thread_id", "950"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "990", "channel_id": "950",
            "author": {"id": "700", "username": "Deploys", "bot": true}, "content": "deployed",
            "timestamp": "2026-10-19T12:00:00+00:00", "flags": 0, "embeds": [], "attachments": []})))
        .mount(&s)
        .await;
    let c = client(&s);
    let msg = OutgoingMessage { content: Some("deployed".to_owned()), ..OutgoingMessage::default() };
    let m = c.execute_webhook(Id(700), &Secret::new(TOKEN), &msg, Some(Id(950)), Some("Deploys"), None).await.unwrap();
    assert_eq!(m.id, Id(990));
    let req =
        s.received_requests().await.unwrap().into_iter().find(|r| r.url.path().contains("/webhooks/700/")).unwrap();
    assert!(req.headers.get("authorization").is_none(), "no bot token on webhook routes");
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["username"], "Deploys");

    let err = c.execute_webhook(Id(701), &Secret::new(TOKEN), &msg, None, None, None).await.unwrap_err();
    assert!(!format!("{err} {err:?}").contains(TOKEN), "{err:?}");
}
