//! T017: name resolution for roles, channels and members (data-model §6).

use guildsmith_app::fake::fixtures;
use guildsmith_app::ports::Member;
use guildsmith_app::resolve::{Resolution, resolve_channel, resolve_member, resolve_role};
use guildsmith_core::Id;

#[test]
fn role_forms_resolve_to_the_same_role() {
    let s = fixtures::lab();
    for input in ["@Moderator", "Moderator", "10", "<@&10>"] {
        assert_eq!(resolve_role(&s, input), Resolution::Resolved(Id(10)), "{input}");
    }
    assert_eq!(resolve_role(&s, "@everyone"), Resolution::Resolved(s.guild.id));
}

#[test]
fn channel_forms_resolve_to_the_same_channel() {
    let s = fixtures::lab();
    for input in ["#general", "general", "Community/general", "101", "<#101>"] {
        assert_eq!(resolve_channel(&s, input), Resolution::Resolved(Id(101)), "{input}");
    }
}

#[test]
fn ambiguous_names_list_candidates() {
    let mut s = fixtures::lab();
    // A second "general" at the top level.
    let mut dup = s.channels.iter().find(|c| c.id == Id(101)).unwrap().clone();
    dup.id = Id(150);
    dup.parent_id = None;
    s.channels.push(dup);
    match resolve_channel(&s, "#general") {
        Resolution::Ambiguous(c) => {
            assert_eq!(c.len(), 2);
            assert!(c.iter().any(|x| x.id == Id(101) && x.parent.as_deref() == Some("Community")));
            assert!(c.iter().any(|x| x.id == Id(150) && x.parent.is_none()));
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
    // A category path still disambiguates.
    assert_eq!(resolve_channel(&s, "Community/general"), Resolution::Resolved(Id(101)));
}

#[test]
fn missing_names_suggest_up_to_three_close_names() {
    let s = fixtures::lab();
    match resolve_role(&s, "@Moderater") {
        Resolution::NotFound { suggestions } => {
            assert!(suggestions.len() <= 3);
            assert_eq!(suggestions.first().map(String::as_str), Some("Moderator"));
        }
        other => panic!("expected not found, got {other:?}"),
    }
    match resolve_channel(&s, "#genral") {
        Resolution::NotFound { suggestions } => {
            assert!(suggestions.contains(&"general".to_owned()));
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[test]
fn members_resolve_by_username_nick_or_id() {
    let members = vec![
        Member {
            id: Id(50),
            username: "alex".into(),
            display_name: Some("Alex P".into()),
            nick: Some("Lexi".into()),
            role_ids: vec![],
            timed_out: false,
        },
        Member {
            id: Id(51),
            username: "sam".into(),
            display_name: None,
            nick: None,
            role_ids: vec![],
            timed_out: false,
        },
    ];
    for input in ["@alex", "alex", "Lexi", "Alex P", "50", "<@50>", "<@!50>"] {
        assert_eq!(resolve_member(&members, input), Resolution::Resolved(Id(50)), "{input}");
    }
    assert!(matches!(resolve_member(&members, "@nobody"), Resolution::NotFound { .. }));
}
