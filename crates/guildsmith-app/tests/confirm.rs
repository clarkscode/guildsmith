//! T069: destructive operations return a preview and change nothing unless confirmed (FR-018).

use guildsmith_app::confirm::{Outcome, guarded};
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::fixtures;
use guildsmith_app::ports::DiscordPort;

#[tokio::test]
async fn without_confirm_it_is_a_preview_and_nothing_is_touched() {
    let fake = fixtures::lab_fake();
    let would = vec!["delete role @Member (11)".to_owned()];
    let out = guarded(false, would.clone(), || async {
        fake.delete_role(fixtures::GUILD, fixtures::MEMBER_ROLE, "guildsmith: delete role @Member").await?;
        Ok::<_, ServiceError>("deleted")
    })
    .await
    .unwrap();
    let Outcome::Preview(p) = &out else { panic!("{out:?}") };
    assert!(p.preview);
    assert_eq!(p.would, would);
    assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::json!({"preview": true, "would": would}));
    assert_eq!(fake.writes(), []);
    assert!(fake.current(fixtures::GUILD).unwrap().role(fixtures::MEMBER_ROLE).is_some());
}

#[tokio::test]
async fn with_confirm_it_runs() {
    let fake = fixtures::lab_fake();
    let out = guarded(true, vec!["delete role @Member (11)".to_owned()], || async {
        fake.delete_role(fixtures::GUILD, fixtures::MEMBER_ROLE, "guildsmith: delete role @Member").await?;
        Ok::<_, ServiceError>("deleted")
    })
    .await
    .unwrap();
    assert_eq!(out, Outcome::Done("deleted"));
    assert_eq!(fake.writes().len(), 1);
    assert!(fake.current(fixtures::GUILD).unwrap().role(fixtures::MEMBER_ROLE).is_none());
}
