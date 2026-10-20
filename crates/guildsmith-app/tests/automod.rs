//! T056: automod validation and the service against `FakeDiscord`: rule counts per type,
//! list sizes and lengths, mention limit, exemptions, timeout only on keyword and mention-spam
//! rules, and confirmation to disable or delete.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::Policy;
use guildsmith_app::services::automod::{self, RuleInput};
use guildsmith_core::spec::automod::{Action, AutomodRule, Keywords, Preset, Trigger, problems};

const G: Option<&str> = Some("1000");

fn app() -> Arc<App<guildsmith_app::fake::FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

fn words(n: usize, len: usize) -> Vec<String> {
    (0..n).map(|i| format!("{i:0len$}")).collect()
}

fn keyword_rule(name: &str) -> RuleInput {
    RuleInput {
        name: Some(name.to_owned()),
        trigger: Some(Trigger::Keyword(Keywords { keywords: vec!["spam".to_owned()], ..Keywords::default() })),
        actions: Some(vec![Action::Block { message: None }, Action::Alert { channel: "#mod-log".to_owned() }]),
        ..RuleInput::default()
    }
}

fn rule(trigger: Trigger, actions: Vec<Action<String>>) -> AutomodRule<String> {
    AutomodRule {
        id: None,
        name: "r".to_owned(),
        enabled: true,
        trigger,
        actions,
        exempt_roles: Vec::new(),
        exempt_channels: Vec::new(),
    }
}

#[test]
fn list_sizes_and_lengths_name_the_limit() {
    let block = vec![Action::Block { message: None }];
    let kw =
        |k: Vec<String>, r: Vec<String>, a: Vec<String>| Trigger::Keyword(Keywords { keywords: k, regex: r, allow: a });
    let cases = [
        (kw(words(1001, 3), vec![], vec![]), "1000 keywords"),
        (kw(words(1, 61), vec![], vec![]), "1–60 characters"),
        (kw(vec![], words(11, 3), vec![]), "10 regex"),
        (kw(vec![], words(1, 261), vec![]), "1–260"),
        (kw(words(1, 3), vec![], words(101, 3)), "100 allowed"),
        (Trigger::Preset { presets: vec![Preset::Slurs], allow: words(1001, 4) }, "1000 allowed"),
        (Trigger::MentionSpam { limit: 51, raid_protection: false }, "1–50"),
    ];
    for (t, want) in cases {
        let p = problems(&rule(t, block.clone()));
        assert!(p.iter().any(|x| x.contains(want)), "{want}: {p:?}");
    }
    let preset = rule(Trigger::Preset { presets: vec![Preset::Profanity], allow: words(1000, 4) }, block.clone());
    assert_eq!(problems(&preset), Vec::<String>::new());
    let mut many = rule(Trigger::Spam, block.clone());
    many.exempt_roles = words(21, 2);
    many.exempt_channels = words(51, 2);
    let p = problems(&many);
    assert!(
        p.iter().any(|x| x.contains("20 exempt roles")) && p.iter().any(|x| x.contains("50 exempt channels")),
        "{p:?}"
    );
}

#[test]
fn timeout_only_on_keyword_and_mention_spam_and_at_most_28_days() {
    let t = |d| vec![Action::Timeout { duration: d }];
    assert!(
        problems(&rule(Trigger::Spam, t(60))).iter().any(|p| p.contains("only allowed on keyword and mention-spam"))
    );
    assert_eq!(
        problems(&rule(Trigger::MentionSpam { limit: 5, raid_protection: true }, t(2_419_200))),
        Vec::<String>::new()
    );
    assert!(
        problems(&rule(Trigger::MentionSpam { limit: 5, raid_protection: true }, t(2_419_201)))
            .iter()
            .any(|p| p.contains("2419200"))
    );
    let kw = Trigger::Keyword(Keywords { keywords: vec!["x".to_owned()], ..Keywords::default() });
    assert_eq!(problems(&rule(kw, t(60))), Vec::<String>::new());
    let profile = Trigger::MemberProfile(Keywords { keywords: vec!["x".to_owned()], ..Keywords::default() });
    assert_eq!(problems(&rule(profile.clone(), vec![Action::BlockInteraction])), Vec::<String>::new());
    assert_ne!(problems(&rule(profile, vec![Action::Block { message: None }])), Vec::<String>::new());
    assert_ne!(problems(&rule(Trigger::Spam, vec![Action::BlockInteraction])), Vec::<String>::new());
}

#[tokio::test]
async fn at_most_6_keyword_rules_and_1_of_each_other_type() {
    let app = app();
    for i in 0..6 {
        let r = automod::automod_rule_upsert(&app, G, None, &keyword_rule(&format!("k{i}")), false, None).await;
        assert!(matches!(r, Ok(Outcome::Done(_))), "{r:?}");
    }
    let err = automod::automod_rule_upsert(&app, G, None, &keyword_rule("k6"), false, None).await.unwrap_err();
    assert!(err.to_string().contains("at most 6 keyword"), "{err}");
    let spam = RuleInput {
        name: Some("spam".to_owned()),
        trigger: Some(Trigger::Spam),
        actions: Some(vec![Action::Block { message: None }]),
        ..RuleInput::default()
    };
    automod::automod_rule_upsert(&app, G, None, &spam, false, None).await.unwrap();
    let again = RuleInput { name: Some("spam 2".to_owned()), ..spam };
    assert!(
        automod::automod_rule_upsert(&app, G, None, &again, false, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("at most 1 spam")
    );
    // Nothing over the limit was sent.
    assert_eq!(app.port.writes().iter().filter(|w| w.method == "create_automod").count(), 7);
}

#[tokio::test]
async fn updates_send_only_changes_disabling_and_deleting_need_confirm() {
    let app = app();
    let Outcome::Done(created) =
        automod::automod_rule_upsert(&app, G, None, &keyword_rule("invites"), false, None).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(created.rule.actions[1], Action::Alert { channel: "#mod-log".to_owned() });
    let same =
        automod::automod_rule_upsert(&app, G, Some("invites"), &RuleInput::default(), false, None).await.unwrap();
    let Outcome::Done(same) = same else { panic!() };
    assert_eq!(same.changes, Vec::<String>::new());
    let off = RuleInput { enabled: Some(false), ..RuleInput::default() };
    assert!(automod::automod_rule_upsert(&app, G, Some("invites"), &off, false, None).await.unwrap().is_preview());
    let Outcome::Done(u) = automod::automod_rule_upsert(&app, G, Some("invites"), &off, true, None).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(u.changes, ["enabled true → false"]);
    let wrong = RuleInput { trigger: Some(Trigger::Spam), ..RuleInput::default() };
    assert!(
        automod::automod_rule_upsert(&app, G, Some("invites"), &wrong, false, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("can't change")
    );
    let exempt = RuleInput { exempt_channels: Some(vec!["#nope".to_owned()]), ..RuleInput::default() };
    assert!(matches!(
        automod::automod_rule_upsert(&app, G, Some("invites"), &exempt, false, None).await,
        Err(ServiceError::NotFound { .. })
    ));
    assert!(automod::automod_rule_delete(&app, G, "invites", false, None).await.unwrap().is_preview());
    automod::automod_rule_delete(&app, G, "invites", true, None).await.unwrap();
    assert_eq!(automod::automod_rules_list(&app, G).await.unwrap(), []);
}
