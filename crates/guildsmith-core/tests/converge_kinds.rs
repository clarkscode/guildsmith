//! T087: every kind beyond roles and channels converges: planning a spec, applying the plan
//! with the pure simulator, and planning again gives no ops (data-model §5). Invites without a
//! code match by signature, messages without an id by exact content and embeds.

mod common;

use common::kinds::{self, NOW};
use guildsmith_core::diff_with;
use guildsmith_core::live::{InviteInfo, LiveExtras};
use guildsmith_core::planner::simulate::simulate_with;
use guildsmith_core::planner::{OpKind, Plan, Written};
use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::spec::GuildSpec;
use proptest::prelude::*;

fn plan(spec: &GuildSpec, s: &Snapshot, x: &LiveExtras) -> Plan {
    diff_with(spec, s, x, &kinds::assets(spec)).unwrap_or_else(|e| panic!("{e}"))
}

/// Applies the spec's plan and returns the server after it, checking the second plan is empty.
fn converge(spec: &GuildSpec, s: &Snapshot, x: &LiveExtras) -> (Snapshot, LiveExtras) {
    let first = plan(spec, s, x);
    let (s2, x2) = simulate_with(s, x, &first.ops).unwrap_or_else(|e| panic!("{e}"));
    let second = plan(spec, &s2, &x2);
    assert!(
        second.ops.is_empty(),
        "not converged; still:\n{}",
        second.ops.iter().map(|o| o.summary.as_str()).collect::<Vec<_>>().join("\n")
    );
    (s2, x2)
}

#[test]
fn a_spec_with_every_kind_converges() {
    let spec = kinds::parse(&kinds::full_text());
    let first = plan(&spec, &common::lab(), &kinds::empty());
    let names: Vec<&str> = first.ops.iter().map(|o| o.summary.as_str()).collect();
    for want in [
        "update server settings",
        "create automod rule No invite links",
        "set onboarding",
        "set the welcome screen",
        "create emoji guildsmith",
        "create sticker Hammer",
        "create sound ding",
        "create webhook CI",
        "create invite",
        "create event Game night",
        "post message",
    ] {
        assert!(names.iter().any(|n| n.starts_with(want)), "{want} missing from {names:#?}");
    }
    converge(&spec, &common::lab(), &kinds::empty());
}

/// Each kind on its own, so a failure names it.
#[test]
fn each_kind_converges_alone() {
    let full = kinds::parse(&kinds::full_text());
    let base = GuildSpec {
        guild: full.guild.clone(),
        roles: full.roles.clone(),
        categories: full.categories.clone(),
        channels: full.channels.clone(),
        ..GuildSpec::default()
    };
    let cases: Vec<(&str, GuildSpec)> = vec![
        ("automod", GuildSpec { automod: full.automod.clone(), ..base.clone() }),
        ("onboarding", GuildSpec { onboarding: full.onboarding.clone(), ..base.clone() }),
        ("welcome", GuildSpec { welcome_screen: full.welcome_screen.clone(), ..base.clone() }),
        ("emoji", GuildSpec { emoji: full.emoji.clone(), ..base.clone() }),
        ("stickers", GuildSpec { stickers: full.stickers.clone(), ..base.clone() }),
        ("sounds", GuildSpec { sounds: full.sounds.clone(), ..base.clone() }),
        ("webhooks", GuildSpec { webhooks: full.webhooks.clone(), ..base.clone() }),
        ("invites", GuildSpec { invites: full.invites.clone(), ..base.clone() }),
        ("events", GuildSpec { events: full.events.clone(), ..base.clone() }),
        ("messages", GuildSpec { messages: full.messages.clone(), ..base.clone() }),
        ("settings", base.clone()),
    ];
    for (name, spec) in cases {
        eprintln!("kind: {name}");
        converge(&spec, &common::lab(), &kinds::empty());
    }
}

#[test]
fn ids_matched_or_created_are_written_back_and_then_nothing() {
    let spec = kinds::parse(&kinds::full_text());
    let first = plan(&spec, &common::lab(), &kinds::empty());
    let paths: Vec<String> = first.write_ids.iter().map(|w| format!("{}.{}", w.path, w.field)).collect();
    for p in ["automod[0].id", "emoji[0].id", "webhooks[0].id", "invites[0].code", "events[0].id", "messages[0].id"] {
        assert!(paths.contains(&p.to_owned()), "{p} missing from {paths:?}");
    }
    assert!(first.write_ids.iter().all(|w| matches!(w.value, Written::New(_))), "all new on an empty server");
}

#[test]
fn invites_without_a_code_match_by_signature_and_messages_by_content() {
    let spec = kinds::parse(&kinds::full_text());
    let (s, x) = converge(&spec, &common::lab(), &kinds::empty());
    let again = plan(&spec, &s, &x);
    let code = x.invites.as_ref().unwrap()[0].code.clone();
    let invite = again.write_ids.iter().find(|w| w.path.to_string() == "invites[0]").unwrap();
    assert_eq!(invite.value, Written::Code(code), "matched by signature, its code written back");
    let message = again.write_ids.iter().find(|w| w.path.to_string() == "messages[0]").unwrap();
    assert!(matches!(message.value, Written::Id(_)), "matched by exact content");

    // A different signature is a different invite: a new one is made.
    let mut other = spec.clone();
    other.invites[0].max_uses = 5;
    let p = plan(&other, &s, &x);
    assert!(p.ops.iter().any(|o| matches!(o.kind, OpKind::CreateInvite { .. })), "{:?}", p.ops);
}

#[test]
fn expired_invites_are_recreated_only_with_recreate() {
    let mut spec = kinds::parse(&kinds::full_text());
    spec.invites[0].code = Some("gone".to_owned());
    let x = kinds::empty();
    let p = plan(&spec, &common::lab(), &x);
    assert!(!p.ops.iter().any(|o| matches!(o.kind, OpKind::CreateInvite { .. })));
    assert!(p.warnings.iter().any(|w| w.contains("recreate = true")), "{:?}", p.warnings);
    spec.invites[0].recreate = true;
    let p = plan(&spec, &common::lab(), &x);
    assert!(p.ops.iter().any(|o| matches!(o.kind, OpKind::CreateInvite { .. })));

    // An invite that has run out counts as gone.
    let mut x = kinds::empty();
    x.invites = Some(vec![InviteInfo {
        code: "old".to_owned(),
        channel_id: guildsmith_core::Id(101),
        inviter_id: Some(common::BOT),
        inviter: None,
        max_age: 60,
        max_uses: 0,
        uses: 0,
        temporary: false,
        created_at: String::new(),
        expires_at: Some("2026-01-01T00:00:00Z".to_owned()),
        roles: Vec::new(),
    }]);
    spec.invites[0].code = Some("old".to_owned());
    spec.invites[0].recreate = false;
    let p = plan(&spec, &common::lab(), &x);
    assert!(p.warnings.iter().any(|w| w.contains("expired")), "{:?}", p.warnings);
}

#[test]
fn past_and_finished_events_are_left_alone() {
    let mut spec = kinds::parse(&kinds::full_text());
    spec.events[0].start = guildsmith_core::spec::events::SpecTime::utc(NOW - 3600);
    spec.events[0].end = Some(guildsmith_core::spec::events::SpecTime::utc(NOW - 60));
    let p = plan(&spec, &common::lab(), &kinds::empty());
    assert!(!p.ops.iter().any(|o| matches!(o.kind, OpKind::CreateEvent { .. })));
    assert!(p.warnings.iter().any(|w| w.contains("in the past")), "{:?}", p.warnings);
}

#[test]
fn active_events_change_only_description_and_end() {
    let spec = kinds::parse(&kinds::full_text());
    let (s, mut x) = converge(&spec, &common::lab(), &kinds::empty());
    for e in x.events.iter_mut().flatten() {
        e.status = "active".to_owned();
    }
    let mut changed = spec.clone();
    // Apply wrote the id back, so a rename keeps the event.
    changed.events[0].id = Some(x.events.as_ref().unwrap()[0].id);
    changed.events[0].name = "Game night!".to_owned();
    changed.events[0].description = Some("Bring snacks".to_owned());
    let p = plan(&changed, &s, &x);
    assert_eq!(p.ops.len(), 1, "{:?}", p.ops);
    assert!(p.ops[0].summary.contains("description") && !p.ops[0].summary.contains("name"), "{}", p.ops[0].summary);
    assert!(p.warnings.iter().any(|w| w.contains("has started")), "{:?}", p.warnings);
}

/// Small edits to every kind, applied to a server already built from the full spec.
fn edited(seed: u64) -> GuildSpec {
    let mut s = kinds::parse(&kinds::full_text());
    let bit = |n: u32| seed & (1 << n) != 0;
    if bit(0) {
        s.automod[0].enabled = false;
    }
    if bit(1) {
        s.emoji[0].roles.clear();
    }
    if bit(2) {
        s.emoji[0].image = guildsmith_core::asset::Asset::Data { data: kinds::png_uri(9) };
    }
    if bit(3) {
        "tools".clone_into(&mut s.stickers[0].tags);
    }
    if bit(4) {
        "Deploys".clone_into(&mut s.webhooks[0].name);
    }
    if bit(5) {
        "#help".clone_into(&mut s.webhooks[0].channel);
    }
    if bit(6) {
        s.events[0].description = Some("Bring snacks".to_owned());
    }
    if bit(7) {
        s.messages[0].content = Some("1. Be kind.".to_owned());
    }
    if bit(8) {
        s.messages[0].pinned = false;
    }
    if bit(9) {
        "Why are you here?".clone_into(&mut s.onboarding.as_mut().unwrap().prompts[0].title);
    }
    if bit(10) {
        s.welcome_screen.as_mut().unwrap().description = Some("Hi!".to_owned());
    }
    if bit(11) {
        s.guild.settings.as_mut().unwrap().afk_timeout = Some(900);
    }
    if bit(12) {
        s.invites[0].max_uses = 10;
    }
    if bit(13) {
        s.prune.automod = true;
        s.automod.clear();
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn edits_converge(seed in any::<u64>()) {
        let full = kinds::parse(&kinds::full_text());
        let (s, x) = converge(&full, &common::lab(), &kinds::empty());
        converge(&edited(seed), &s, &x);
    }
}
