//! T035: the threads service against `FakeDiscord`: names 1–100, auto-archive in
//! 60/1440/4320/10080, forum tags (≤ 20 per forum, ≤ 5 per post), unknown tags listed, and
//! tag-required forums refusing untagged posts.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::threads::REQUIRE_TAG;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::messages::ComposeInput;
use guildsmith_app::services::threads::{self, MemberAction, TagAction, ThreadInput, ThreadState, ThreadUpdate};
use guildsmith_core::Id;

const G: Option<&str> = Some("1000");
const HELP: Id = Id(106);

fn app() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())]), Policy::default()))
}

fn thread(name: &str) -> ThreadInput {
    ThreadInput { name: name.to_owned(), ..ThreadInput::default() }
}

fn post(name: &str, tags: &[&str]) -> ThreadInput {
    ThreadInput {
        name: name.to_owned(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        message: Some(ComposeInput { content: Some("How do I…?".to_owned()), ..ComposeInput::default() }),
        ..ThreadInput::default()
    }
}

#[tokio::test]
async fn names_and_auto_archive_are_checked() {
    let app = app();
    for bad in [thread(""), thread(&"x".repeat(101))] {
        assert!(matches!(threads::thread_create(&app, G, "#general", &bad, None).await, Err(ServiceError::Invalid(_))));
    }
    let odd = ThreadInput { auto_archive: Some(120), ..thread("ok") };
    let err = threads::thread_create(&app, G, "#general", &odd, None).await.unwrap_err();
    assert!(err.to_string().contains("60, 1440, 4320 or 10080"), "{err}");
    for minutes in [60, 1440, 4320, 10_080] {
        let t = ThreadInput { auto_archive: Some(minutes), ..thread(&format!("t{minutes}")) };
        assert_eq!(threads::thread_create(&app, G, "#general", &t, None).await.unwrap().auto_archive, minutes);
    }
    assert_eq!(app.port.writes().iter().filter(|w| w.method == "create_thread").count(), 4);
}

#[tokio::test]
async fn forum_posts_take_known_tags_up_to_5_and_unknown_ones_are_listed() {
    let app = app();
    let created = threads::thread_create(&app, G, "#help", &post("Login fails", &["question"]), None).await.unwrap();
    assert_eq!(created.tags, ["question"]);
    let err = threads::thread_create(&app, G, "#help", &post("x", &["bug"]), None).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown tag `bug`") && msg.contains("question") && msg.contains("solved"), "{msg}");
    let six = ["question", "solved", "question", "solved", "question", "solved"];
    assert!(
        threads::thread_create(&app, G, "#help", &post("x", &six), None)
            .await
            .unwrap_err()
            .to_string()
            .contains("at most 5")
    );
    // A forum post needs its first message.
    assert!(threads::thread_create(&app, G, "#help", &thread("no message"), None).await.is_err());
    // Tags are for forums only.
    assert!(threads::thread_create(&app, G, "#general", &post("x", &["question"]), None).await.is_err());
}

#[tokio::test]
async fn a_tag_required_forum_refuses_untagged_posts() {
    let app = app();
    app.port.set_channel_flags(fixtures::GUILD, HELP, REQUIRE_TAG);
    let err = threads::thread_create(&app, G, "#help", &post("untagged", &[]), None).await.unwrap_err();
    assert!(err.to_string().contains("requires a tag") && err.to_string().contains("question"), "{err}");
    assert!(threads::thread_create(&app, G, "#help", &post("tagged", &["solved"]), None).await.is_ok());
}

#[tokio::test]
async fn forums_hold_at_most_20_tags_and_deleting_one_previews() {
    let app = app();
    for i in 0..18 {
        let r = threads::forum_tags(
            &app,
            G,
            "#help",
            TagAction::Create,
            None,
            Some(&format!("tag{i}")),
            None,
            None,
            false,
            None,
        )
        .await;
        assert!(matches!(r, Ok(Outcome::Done(_))), "{r:?}");
    }
    let err = threads::forum_tags(&app, G, "#help", TagAction::Create, None, Some("one more"), None, None, false, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("at most 20"), "{err}");
    let del = threads::forum_tags(&app, G, "#help", TagAction::Delete, Some("tag0"), None, None, None, false, None)
        .await
        .unwrap();
    assert!(del.is_preview());
    let Outcome::Done(t) =
        threads::forum_tags(&app, G, "#help", TagAction::Delete, Some("tag0"), None, None, None, true, None)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(t.tags.len(), 19);
    let snapshot = app.port.current(fixtures::GUILD).unwrap();
    let help = snapshot.channel(HELP).unwrap();
    assert!(help.tags.iter().any(|t| t.id == Id(201) && t.name == "question"), "existing tags keep their ids");
    let Outcome::Done(r) =
        threads::forum_tags(&app, G, "#help", TagAction::DefaultReaction, None, None, Some("🔥"), None, false, None)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(r.default_reaction.as_deref(), Some("🔥"));
}

#[tokio::test]
async fn update_list_members_and_delete() {
    let app = app();
    let t = threads::thread_create(&app, G, "#general", &thread("weekly update"), None).await.unwrap();
    let u = threads::thread_update(
        &app,
        G,
        "weekly update",
        &ThreadUpdate { archived: Some(true), locked: Some(true), ..ThreadUpdate::default() },
        None,
    )
    .await
    .unwrap();
    assert_eq!(u.changes.len(), 2);
    // Archived threads aren't found by name among the active ones, but are by id.
    let by_name = threads::thread_update(
        &app,
        G,
        "weekly update",
        &ThreadUpdate { archived: Some(false), ..ThreadUpdate::default() },
        None,
    )
    .await;
    assert!(matches!(by_name, Err(ServiceError::NotFound { .. })));
    let same = threads::thread_update(
        &app,
        G,
        &t.id.to_string(),
        &ThreadUpdate { archived: Some(true), ..ThreadUpdate::default() },
        None,
    )
    .await
    .unwrap();
    assert_eq!(same.changes, Vec::<String>::new());
    let archived =
        threads::threads_list(&app, G, Some("#general"), ThreadState::ArchivedPublic, None, None).await.unwrap();
    assert_eq!(archived.threads.len(), 1);
    assert!(
        threads::threads_list(&app, G, None, ThreadState::ArchivedPublic, None, None).await.is_err(),
        "archived needs a channel"
    );

    let t2 = threads::thread_create(&app, G, "#general", &thread("team"), None).await.unwrap();
    let m = threads::thread_members(&app, G, "team", MemberAction::Add, Some("sam")).await.unwrap();
    assert!(m.members.contains(&fixtures::SAM));
    let m = threads::thread_members(&app, G, "team", MemberAction::Remove, Some("sam")).await.unwrap();
    assert!(!m.members.contains(&fixtures::SAM));
    assert!(threads::thread_delete(&app, G, "team", false, None).await.unwrap().is_preview());
    threads::thread_delete(&app, G, &t2.id.to_string(), true, None).await.unwrap();
    assert!(threads::thread_delete(&app, G, &t2.id.to_string(), true, None).await.is_err(), "gone");
}
