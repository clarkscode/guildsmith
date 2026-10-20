//! T069: the expressions service against `FakeDiscord`: name rules per kind, sticker text,
//! sound volume, Lottie stickers only on verified/partnered servers, oversized files refused
//! before upload, and the approximate slot table.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::ExpressionKind;
use guildsmith_app::services::expressions::{self, ExpressionInput, ExpressionUpdate, approx_slots};
use guildsmith_core::asset::{Asset, FileType, data_uri};

const G: Option<&str> = Some("1000");

fn app_with(features: &[&str], tier: u8) -> Arc<App<FakeDiscord>> {
    let mut s = fixtures::lab();
    s.guild.premium_tier = tier;
    s.guild.features.extend(features.iter().map(|f| (*f).to_owned()));
    Arc::new(App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), Policy::default()))
}

fn png(size: usize) -> Asset {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    b.extend(b"IDAT");
    b.resize(size, 1);
    Asset::Data { data: data_uri(FileType::Png, &b) }
}

fn mp3(frames: usize) -> Asset {
    let mut f = vec![0xFF, 0xFB, 0x90, 0x64];
    f.resize(417, 0);
    Asset::Data { data: data_uri(FileType::Mp3, &f.repeat(frames)) }
}

fn lottie() -> Asset {
    Asset::Data { data: data_uri(FileType::Lottie, br#"{"v":"5.7.4","layers":[]}"#) }
}

fn input(kind: ExpressionKind, name: &str, asset: Asset) -> ExpressionInput {
    ExpressionInput {
        kind,
        name: name.to_owned(),
        asset,
        roles: Vec::new(),
        description: None,
        tags: (kind == ExpressionKind::Sticker).then(|| "👋".to_owned()),
        volume: None,
        emoji: None,
    }
}

fn invalid(r: &Result<impl std::fmt::Debug, ServiceError>, want: &str) {
    match r {
        Err(ServiceError::Invalid(m)) => assert!(m.contains(want), "{want}: {m}"),
        other => panic!("expected invalid ({want}), got {other:?}"),
    }
}

#[tokio::test]
async fn names_text_and_volume_follow_discords_rules() {
    let app = app_with(&[], 0);
    for bad in ["a", &"a".repeat(33), "no-dashes", "no spaces"] {
        invalid(
            &expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, bad, png(64)), None).await,
            "2–32",
        );
    }
    assert!(
        expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, "logo_2", png(64)), None).await.is_ok()
    );
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Sticker, &"s".repeat(31), png(64)), None).await,
        "2–30",
    );
    let mut st = input(ExpressionKind::Sticker, "wave", png(64));
    st.description = Some("x".to_owned());
    invalid(&expressions::expression_create(&app, G, &st, None).await, "2–100");
    st.description = Some(String::new());
    st.tags = Some("t".repeat(201));
    invalid(&expressions::expression_create(&app, G, &st, None).await, "200");
    st.tags = Some("👋".to_owned());
    assert!(expressions::expression_create(&app, G, &st, None).await.is_ok());
    let mut snd = input(ExpressionKind::Sound, "horn", mp3(100));
    snd.volume = Some(1.5);
    invalid(&expressions::expression_create(&app, G, &snd, None).await, "0–1");
    snd.volume = Some(0.5);
    assert!(expressions::expression_create(&app, G, &snd, None).await.is_ok());
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Sound, &"s".repeat(33), mp3(10)), None).await,
        "2–32",
    );
}

#[tokio::test]
async fn lottie_stickers_need_a_verified_or_partnered_server() {
    let plain = app_with(&[], 0);
    invalid(
        &expressions::expression_create(&plain, G, &input(ExpressionKind::Sticker, "anim", lottie()), None).await,
        "verified or partnered",
    );
    let verified = app_with(&["VERIFIED"], 0);
    assert!(
        expressions::expression_create(&verified, G, &input(ExpressionKind::Sticker, "anim", lottie()), None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn oversized_files_are_refused_before_upload() {
    let app = app_with(&[], 0);
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, "big", png(256 * 1024 + 1)), None).await,
        "256 KiB",
    );
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Sticker, "big", png(512 * 1024 + 1)), None)
            .await,
        "512 KiB",
    );
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Sound, "long", mp3(250)), None).await,
        "5.2 s",
    );
    invalid(
        &expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, "notimg", mp3(5)), None).await,
        "JPEG/PNG",
    );
    assert_eq!(app.port.writes(), [], "nothing was uploaded");
}

#[tokio::test]
async fn slots_follow_the_approximate_table() {
    assert_eq!([0, 1, 2, 3].map(|t| approx_slots(ExpressionKind::Emoji, t)), [50, 100, 150, 250]);
    assert_eq!([0, 1, 2, 3].map(|t| approx_slots(ExpressionKind::Sticker, t)), [5, 15, 30, 60]);
    assert_eq!([0, 1, 2, 3].map(|t| approx_slots(ExpressionKind::Sound, t)), [8, 24, 36, 48]);
    let app = app_with(&[], 2);
    expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, "one", png(64)), None).await.unwrap();
    let list = expressions::expressions_list(&app, G, None).await.unwrap();
    let emoji = list.slots.iter().find(|s| s.kind == "emoji (static)").unwrap();
    assert_eq!((emoji.used, emoji.approx_max, emoji.approx_remaining), (1, 150, 149));
    assert!(list.note.contains("approximately"));
    assert_eq!(list.slots.len(), 4);
}

#[tokio::test]
async fn rename_and_delete() {
    let app = app_with(&[], 0);
    expressions::expression_create(&app, G, &input(ExpressionKind::Emoji, "old_logo", png(64)), None).await.unwrap();
    let u = expressions::expression_update(
        &app,
        G,
        ExpressionKind::Emoji,
        ":old_logo:",
        &ExpressionUpdate {
            name: Some("logo".to_owned()),
            roles: Some(vec!["@Member".to_owned()]),
            ..ExpressionUpdate::default()
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(u.changes.len(), 2);
    assert_eq!(u.item.roles, ["@Member"]);
    assert!(
        expressions::expression_delete(&app, G, ExpressionKind::Emoji, "logo", false, None).await.unwrap().is_preview()
    );
    expressions::expression_delete(&app, G, ExpressionKind::Emoji, "logo", true, None).await.unwrap();
    assert_eq!(expressions::expressions_list(&app, G, Some(ExpressionKind::Emoji)).await.unwrap().items, []);
}
