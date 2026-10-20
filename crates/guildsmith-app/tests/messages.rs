//! T034: the messages service against `FakeDiscord` (US2, research E4/E11).

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::{Member, MessagesPort};
use guildsmith_app::services::messages::{self, ComposeInput, MentionsInput, ReactionAction, SearchInput};
use guildsmith_core::Id;
use guildsmith_core::asset::{Asset, FileType, data_uri};
use serde_json::json;

const G: Option<&str> = Some("1000");

fn app() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())]), Policy::default()))
}

fn sam() -> Member {
    fixtures::lab_members().into_iter().find(|m| m.id == fixtures::SAM).unwrap()
}

fn text(t: &str) -> ComposeInput {
    ComposeInput { content: Some(t.to_owned()), ..ComposeInput::default() }
}

#[tokio::test]
async fn only_the_bots_own_content_can_be_edited_and_others_get_flags_only() {
    let app = app();
    let mine = messages::message_compose(&app, G, "#general", &text("hello")).await.unwrap();
    let edited = messages::message_edit(&app, G, "#general", mine.id, Some("hello again"), None, None).await.unwrap();
    assert_eq!(edited.changes, ["content"]);
    let same = messages::message_edit(&app, G, "#general", mine.id, Some("hello again"), None, None).await.unwrap();
    assert_eq!(same.changes, Vec::<String>::new(), "no request when nothing changes");

    app.port.post_as_user(fixtures::GUILD, fixtures::GENERAL, &sam(), "see https://example.com");
    let theirs = app.port.recent(fixtures::GENERAL, 1, None).await.unwrap()[0].id;
    let err = messages::message_edit(&app, G, "#general", theirs, Some("hijacked"), None, None).await.unwrap_err();
    assert!(err.to_string().contains("only messages the bot wrote"), "{err}");
    let flags = messages::message_edit(&app, G, "#general", theirs, None, None, Some(true)).await.unwrap();
    assert_eq!(flags.changes, ["suppress_embeds on"]);
    assert_eq!(
        app.port.message(fixtures::GENERAL, theirs).await.unwrap().flags & messages::SUPPRESS_EMBEDS,
        messages::SUPPRESS_EMBEDS
    );
}

#[tokio::test]
async fn pins_are_limited_to_250_per_channel() {
    let app = app();
    let first = messages::message_compose(&app, G, "#general", &text("rules")).await.unwrap();
    let p = messages::message_pin(&app, G, "#general", first.id, true, None).await.unwrap();
    assert!(p.changed && p.pinned);
    assert!(!messages::message_pin(&app, G, "#general", first.id, true, None).await.unwrap().changed, "already pinned");
    let page = messages::pins_list(&app, G, "#general", None, None).await.unwrap();
    assert_eq!(page.pins.len(), 1);
    for i in 0..249 {
        let m = messages::message_compose(&app, G, "#general", &text(&format!("m{i}"))).await.unwrap();
        messages::message_pin(&app, G, "#general", m.id, true, None).await.unwrap();
    }
    let extra = messages::message_compose(&app, G, "#general", &text("one too many")).await.unwrap();
    let err = messages::message_pin(&app, G, "#general", extra.id, true, None).await.unwrap_err();
    assert!(err.to_string().contains("250"), "{err}");
}

#[tokio::test]
async fn components_v2_rules() {
    let app = app();
    let layout = json!([{"type": 10, "content": "Hello"}]);
    let ok = ComposeInput { components_v2: Some(layout.clone()), ..ComposeInput::default() };
    let sent = messages::message_compose(&app, G, "#general", &ok).await.unwrap();
    assert_eq!(
        app.port.message(fixtures::GENERAL, sent.id).await.unwrap().flags & messages::COMPONENTS_V2,
        messages::COMPONENTS_V2
    );
    for bad in [
        ComposeInput { content: Some("x".to_owned()), components_v2: Some(layout.clone()), ..ComposeInput::default() },
        ComposeInput {
            embeds: vec![json!({"title": "x"})],
            components_v2: Some(layout.clone()),
            ..ComposeInput::default()
        },
    ] {
        assert!(matches!(messages::message_compose(&app, G, "#general", &bad).await, Err(ServiceError::Invalid(_))));
    }
    let many: Vec<_> = (0..41).map(|i| json!({"type": 10, "content": format!("{i}")})).collect();
    let too_many = ComposeInput { components_v2: Some(json!(many)), ..ComposeInput::default() };
    let err = messages::message_compose(&app, G, "#general", &too_many).await.unwrap_err();
    assert!(err.to_string().contains("40 components"), "{err}");
}

#[tokio::test]
async fn mentions_default_to_none() {
    let app = app();
    let input = ComposeInput { content: Some("@everyone hi".to_owned()), ..ComposeInput::default() };
    let msg = messages::build_message_for_test(&app, &input).await.unwrap();
    assert!(
        !msg.allowed_mentions.everyone
            && msg.allowed_mentions.users.is_empty()
            && msg.allowed_mentions.roles.is_empty()
    );
    let input = ComposeInput {
        content: Some("@Moderator @sam".to_owned()),
        allow_mentions: Some(MentionsInput {
            users: vec!["sam".to_owned()],
            roles: vec!["@Moderator".to_owned()],
            everyone: false,
        }),
        ..ComposeInput::default()
    };
    let msg = messages::build_message_for_test(&app, &input).await.unwrap();
    assert_eq!(
        (msg.allowed_mentions.users, msg.allowed_mentions.roles),
        (vec![fixtures::SAM], vec![fixtures::MODERATOR])
    );
}

#[tokio::test]
async fn attachments_are_checked_and_sent() {
    let app = app();
    let png = {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        b.extend(b"IDAT");
        b.resize(64, 1);
        b
    };
    let input = ComposeInput {
        attachments: vec![Asset::Data { data: data_uri(FileType::Png, &png) }],
        ..ComposeInput::default()
    };
    let sent = messages::message_compose(&app, G, "#general", &input).await.unwrap();
    let m = app.port.message(fixtures::GENERAL, sent.id).await.unwrap();
    assert_eq!(m.attachments[0].filename, "attachment1.png");
    let local =
        ComposeInput { attachments: vec![Asset::File { file: "/etc/hostname".to_owned() }], ..ComposeInput::default() };
    assert!(matches!(messages::message_compose(&app, G, "#general", &local).await, Err(ServiceError::Refused(_))));
}

#[tokio::test]
async fn delete_previews_and_clear_needs_confirm() {
    let app = app();
    let sent = messages::message_compose(&app, G, "#general", &text("bye")).await.unwrap();
    let Outcome::Preview(p) = messages::message_delete(&app, G, "#general", sent.id, false, None).await.unwrap() else {
        panic!()
    };
    assert!(p.would[0].contains("\"bye\""), "{:?}", p.would);
    messages::reactions(&app, G, "#general", sent.id, ReactionAction::Add, Some("✅"), None, false).await.unwrap();
    app.port.react_as(fixtures::GUILD, sent.id, fixtures::SAM, "✅");
    let users = messages::reactions_list(&app, G, "#general", sent.id, "✅", None, None).await.unwrap();
    assert_eq!(users.len(), 2);
    let clear =
        messages::reactions(&app, G, "#general", sent.id, ReactionAction::Clear, None, None, false).await.unwrap();
    assert!(clear.is_preview());
    messages::reactions(&app, G, "#general", sent.id, ReactionAction::Clear, None, None, true).await.unwrap();
    assert_eq!(app.port.message(fixtures::GENERAL, sent.id).await.unwrap().reactions, []);
    assert!(matches!(
        messages::message_delete(&app, G, "#general", sent.id, true, None).await.unwrap(),
        Outcome::Done(_)
    ));
}

#[tokio::test]
async fn crosspost_only_in_announcement_channels() {
    let app = app();
    let news = messages::message_compose(&app, G, "#announcements", &text("v1.0 is out")).await.unwrap();
    assert!(messages::message_crosspost(&app, G, "#announcements", news.id).await.is_ok());
    let plain = messages::message_compose(&app, G, "#general", &text("hi")).await.unwrap();
    assert!(messages::message_crosspost(&app, G, "#general", plain.id).await.is_err());
}

#[tokio::test]
async fn search_falls_back_to_a_scan_of_at_most_500_messages_per_channel() {
    let app = app();
    for i in 0..600 {
        app.port.post_as_user(fixtures::GUILD, fixtures::GENERAL, &sam(), &format!("message {i}"));
    }
    app.port.set_not_indexed(fixtures::GUILD, Some(3));
    let r = messages::messages_search(
        &app,
        G,
        &SearchInput { content: Some("message 1".to_owned()), ..SearchInput::default() },
    )
    .await
    .unwrap();
    assert!(!r.indexed);
    assert_eq!(r.retry_after, Some(3));
    let scan = SearchInput {
        content: Some("message 1".to_owned()),
        channels: vec!["#general".to_owned()],
        scan: true,
        ..SearchInput::default()
    };
    let r = messages::messages_search(&app, G, &scan).await.unwrap();
    // Only the newest 500 were read: messages 100..599. "message 1" matches 100-199.
    assert_eq!(r.total, 100, "{:?}", r.notes);
    assert!(r.notes.iter().any(|n| n.contains("500")));
    app.port.set_not_indexed(fixtures::GUILD, None);
    let found = messages::messages_search(
        &app,
        G,
        &SearchInput { content: Some("message 599".to_owned()), ..SearchInput::default() },
    )
    .await
    .unwrap();
    assert_eq!((found.indexed, found.total), (true, 1));
    // Without the intent, the result says why text may be empty.
    app.port.set_app_flags(0);
    let r = messages::messages_search(&app, G, &SearchInput::default()).await.unwrap();
    assert!(r.notes.iter().any(|n| n.contains("MESSAGE_CONTENT")), "{:?}", r.notes);
    let _ = Id(0);
}
