//! T061: webhook tokens and URLs never leak (SC-006). With seeded tokens, list outputs, log
//! lines (captured at trace), errors and held state contain no token or URL; only
//! `webhook_create`'s result carries `secrets.url`.

use std::io::Write;
use std::sync::{Arc, Mutex};

use guildsmith_app::App;
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::WebhookKind;
use guildsmith_app::services::messages::ComposeInput;
use guildsmith_app::services::webhooks;
use tracing_subscriber::fmt::MakeWriter;

const SEEDED: &str = "seeded-webhook-token-9f8e7d6c5b4a";
const G: Option<&str> = Some("1000");

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn leaks(text: &str, tokens: &[&str]) -> Option<String> {
    tokens
        .iter()
        .find(|t| text.contains(*t))
        .map(|t| (*t).to_owned())
        .or_else(|| text.contains("/api/webhooks/").then(|| "a webhook URL".to_owned()))
}

#[tokio::test]
async fn tokens_appear_only_in_the_create_result() {
    let logs = Captured::default();
    let subscriber = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_writer(logs.clone()).finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let app = Arc::new(App::new(fixtures::lab_fake(), Policy::default()));
    let seeded =
        app.port.seed_webhook(fixtures::GUILD, fixtures::GENERAL, "Old CI", WebhookKind::Incoming, Some(SEEDED));
    app.port.seed_webhook(fixtures::GUILD, fixtures::GENERAL, "News", WebhookKind::ChannelFollower, Some(SEEDED));

    let created = webhooks::webhook_create(&app, G, "#general", "CI", None, None).await.unwrap();
    let url = created.secrets.url.clone();
    let token = url.rsplit('/').next().unwrap().to_owned();
    assert!(url.starts_with("https://discord.com/api/webhooks/") && token.len() > 10);
    let tokens = [SEEDED, token.as_str()];

    let mut outputs = Vec::new();
    outputs.push(serde_json::to_string(&created.webhook).unwrap());
    outputs.push(serde_json::to_string(&webhooks::webhooks_list(&app, G, None).await.unwrap()).unwrap());
    outputs.push(serde_json::to_string(&webhooks::webhooks_list(&app, G, Some("#general")).await.unwrap()).unwrap());
    let msg = ComposeInput { content: Some("release notes".to_owned()), ..ComposeInput::default() };
    outputs.push(
        serde_json::to_string(&webhooks::webhook_send(&app, G, "CI", &msg, None, Some("CI bot"), None).await.unwrap())
            .unwrap(),
    );
    outputs.push(
        serde_json::to_string(&webhooks::webhook_update(&app, G, "CI", Some("CI 2"), None, None, None).await.unwrap())
            .unwrap(),
    );
    // Without a token for the seeded webhook (Discord doesn't return it here), sending fails.
    let err = webhooks::webhook_send(&app, G, &seeded.to_string(), &msg, None, None, None).await.unwrap_err();
    outputs.push(err.to_string());
    outputs.push(format!("{err:?}"));
    // Follower webhooks are read-only.
    outputs.push(webhooks::webhook_delete(&app, G, "News", true, None).await.unwrap_err().to_string());
    // Discord returns tokens here: sending works, and the token still never comes back.
    app.port.set_webhook_tokens_returned(fixtures::GUILD, true);
    outputs.push(
        serde_json::to_string(&webhooks::webhook_send(&app, G, "Old CI", &msg, None, None, None).await.unwrap())
            .unwrap(),
    );
    outputs.push(format!("{:?}", app.webhook_tokens));
    outputs.push(format!("{:?}", app.port.writes()));
    outputs
        .push(serde_json::to_string(&webhooks::webhook_delete(&app, G, "CI 2", false, None).await.unwrap()).unwrap());

    for o in &outputs {
        assert_eq!(leaks(o, &tokens), None, "leaked in: {o}");
    }
    let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert_eq!(leaks(&log, &tokens), None, "leaked in the logs");
    assert!(!app.webhook_tokens.is_empty(), "the created webhook's token is held in memory");
}

#[test]
fn the_url_type_is_never_built_from_a_list() {
    // Lists come from `WebhookInfo`, which has no token field at all; this documents it.
    let fields = serde_json::to_value(guildsmith_app::ports::WebhookInfo {
        id: guildsmith_core::Id(1),
        kind: WebhookKind::Incoming,
        name: "x".to_owned(),
        channel_id: guildsmith_core::Id(2),
        creator_id: None,
        creator_name: None,
        source: None,
        avatar: Some("a1b2".to_owned()),
    })
    .unwrap();
    let keys: Vec<&String> = fields.as_object().unwrap().keys().collect();
    assert!(!keys.iter().any(|k| k.contains("token") || k.contains("url")), "{keys:?}");
}
