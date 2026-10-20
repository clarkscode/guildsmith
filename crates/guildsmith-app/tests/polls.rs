//! T036: the polls service against `FakeDiscord`: answers 2–10, question ≤ 300 characters,
//! answers ≤ 55, duration ≤ 768 h, and only the bot's own polls can be ended.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::messages::MessagesState;
use guildsmith_app::fake::polls::FakePoll;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::MessagesPort;
use guildsmith_app::services::polls;

const G: Option<&str> = Some("1000");

fn app() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

fn answers(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Day {i}")).collect()
}

#[tokio::test]
async fn limits_are_checked_before_sending() {
    let app = app();
    let bad = [
        (String::new(), answers(2), 24),
        ("x".repeat(301), answers(2), 24),
        ("Which day?".to_owned(), answers(1), 24),
        ("Which day?".to_owned(), answers(11), 24),
        ("Which day?".to_owned(), vec!["a".repeat(56), "b".to_owned()], 24),
        ("Which day?".to_owned(), answers(2), 0),
        ("Which day?".to_owned(), answers(2), 769),
    ];
    for (q, a, h) in bad {
        assert!(
            matches!(polls::poll_create(&app, G, "#general", &q, &a, h, false).await, Err(ServiceError::Invalid(_))),
            "{q} {a:?} {h}"
        );
    }
    assert_eq!(app.port.writes(), []);
    assert!(polls::poll_create(&app, G, "#general", &"x".repeat(300), &answers(10), 768, true).await.is_ok());
}

#[tokio::test]
async fn results_count_votes_and_list_voters_and_only_own_polls_end() {
    let app = app();
    let sent = polls::poll_create(
        &app,
        G,
        "#general",
        "Game night?",
        &["Friday".to_owned(), "Saturday".to_owned()],
        48,
        false,
    )
    .await
    .unwrap();
    app.port.vote(fixtures::GUILD, sent.id, fixtures::SAM, 1);
    app.port.vote(fixtures::GUILD, sent.id, fixtures::ALEX, 1);
    app.port.vote(fixtures::GUILD, sent.id, fixtures::OWNER, 2);
    let r = polls::poll_results(&app, G, "#general", sent.id, Some("friday"), None, None).await.unwrap();
    assert_eq!(r.poll.answers.iter().map(|a| a.votes).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(r.voters.unwrap().len(), 2);
    let err = polls::poll_results(&app, G, "#general", sent.id, Some("Sunday"), None, None).await.unwrap_err();
    assert!(err.to_string().contains("Friday (1)"), "{err}");
    let ended = polls::poll_end(&app, G, "#general", sent.id).await.unwrap();
    assert!(ended.poll.finalized);

    // A poll a member sent: the bot can't end it, and no request is made.
    let sam = fixtures::lab_members().into_iter().find(|m| m.id == fixtures::SAM).unwrap();
    app.port.post_as_user(fixtures::GUILD, fixtures::GENERAL, &sam, "");
    let theirs = app.port.recent(fixtures::GENERAL, 1, None).await.unwrap()[0].id;
    app.port.edit_res::<MessagesState>(fixtures::GUILD, |s| {
        s.extra.entry(theirs).or_default().poll = Some(FakePoll {
            question: "Pizza?".to_owned(),
            answers: vec![("yes".to_owned(), None), ("no".to_owned(), None)],
            ..FakePoll::default()
        });
    });
    let writes = app.port.writes().len();
    let err = polls::poll_end(&app, G, "#general", theirs).await.unwrap_err();
    assert!(err.to_string().contains("only polls the bot sent"), "{err}");
    assert_eq!(app.port.writes().len(), writes);
    app.port.post_as_user(fixtures::GUILD, fixtures::GENERAL, &sam, "not a poll");
    let plain = app.port.recent(fixtures::GENERAL, 1, None).await.unwrap()[0].id;
    assert!(polls::poll_end(&app, G, "#general", plain).await.unwrap_err().to_string().contains("no poll"));
}
