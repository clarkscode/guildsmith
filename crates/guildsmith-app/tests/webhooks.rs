//! T062: the webhooks service against `FakeDiscord`: names 1–80 without "clyde" or "discord"
//! (any case), at most 15 per channel and 1000 per server, follower and application webhooks
//! read-only, and deleting needs confirmation.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::{MessagesPort, WebhookKind};
use guildsmith_app::services::messages::ComposeInput;
use guildsmith_app::services::webhooks;

const G: Option<&str> = Some("1000");

fn app() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

#[tokio::test]
async fn names_are_checked() {
    let app = app();
    for bad in ["", &"x".repeat(81), "Clyde", "my DISCORD bot", "clydeish"] {
        let r = webhooks::webhook_create(&app, G, "#general", bad, None, None).await;
        assert!(matches!(r, Err(ServiceError::Invalid(_))), "{bad:?}: {r:?}");
    }
    assert_eq!(app.port.writes(), []);
    assert!(webhooks::webhook_create(&app, G, "#general", &"x".repeat(80), None, None).await.is_ok());
}

#[tokio::test]
async fn at_most_15_per_channel() {
    let app = app();
    for i in 0..15 {
        webhooks::webhook_create(&app, G, "#general", &format!("hook {i}"), None, None).await.unwrap();
    }
    let err = webhooks::webhook_create(&app, G, "#general", "hook 15", None, None).await.unwrap_err();
    assert!(err.to_string().contains("15 webhooks"), "{err}");
    assert!(webhooks::webhook_create(&app, G, "#mod-log", "elsewhere", None, None).await.is_ok());
}

#[tokio::test]
async fn follower_and_app_webhooks_are_read_only_and_delete_needs_confirm() {
    let app = app();
    app.port.seed_webhook(fixtures::GUILD, fixtures::GENERAL, "Follower", WebhookKind::ChannelFollower, None);
    app.port.seed_webhook(fixtures::GUILD, fixtures::GENERAL, "AppHook", WebhookKind::Application, None);
    for name in ["Follower", "AppHook"] {
        assert!(webhooks::webhook_update(&app, G, name, Some("new"), None, None, None).await.is_err());
        assert!(webhooks::webhook_delete(&app, G, name, true, None).await.is_err());
    }
    let listed = webhooks::webhooks_list(&app, G, None).await.unwrap();
    assert_eq!(
        listed.iter().map(|w| w.kind).collect::<Vec<_>>(),
        [WebhookKind::ChannelFollower, WebhookKind::Application]
    );
    webhooks::webhook_create(&app, G, "#general", "CI", None, None).await.unwrap();
    assert!(webhooks::webhook_delete(&app, G, "CI", false, None).await.unwrap().is_preview());
    assert!(matches!(webhooks::webhook_delete(&app, G, "CI", true, None).await.unwrap(), Outcome::Done(_)));
}

#[tokio::test]
async fn send_rename_and_move() {
    let app = app();
    let created = webhooks::webhook_create(&app, G, "#general", "CI", None, None).await.unwrap();
    let msg = ComposeInput { content: Some("build passed".to_owned()), ..ComposeInput::default() };
    let sent = webhooks::webhook_send(&app, G, "CI", &msg, None, Some("Deploy bot"), None).await.unwrap();
    let m = app.port.message(fixtures::GENERAL, sent.id).await.unwrap();
    assert_eq!(
        (m.author_id, m.author_name.as_str(), m.content.as_str()),
        (created.webhook.id, "Deploy bot", "build passed")
    );
    let moved = webhooks::webhook_update(&app, G, "CI", Some("Builds"), None, Some("#mod-log"), None).await.unwrap();
    assert_eq!(moved.changes.len(), 2);
    assert_eq!(moved.webhook.channel, "#mod-log");
    let again =
        webhooks::webhook_update(&app, G, "Builds", Some("Builds"), None, Some("#mod-log"), None).await.unwrap();
    assert_eq!(again.changes, Vec::<String>::new());
}
