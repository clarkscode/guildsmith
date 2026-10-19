//! T057: export round trip (SC-001): for the recorded fixtures and for generated snapshots,
//! planning the exported text against the same server gives no ops. The text goes through the
//! real writer and parser, so formatting bugs show up here too.

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::{export, plan};
use guildsmith_core::snapshot::{LiveOverwrite, OverwriteKind, Snapshot};
use guildsmith_core::spec::parse;
use guildsmith_core::{Id, PermissionSet, diff};
use proptest::prelude::*;

fn app(s: Snapshot) -> App<FakeDiscord> {
    App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), Policy::default())
}

async fn assert_round_trip(s: Snapshot) {
    let app = app(s);
    let out = export::export(&app, Some("1000"), "2026-10-19").await.unwrap();
    let p = plan::plan(&app, Some("1000"), &out.toml).await.unwrap_or_else(|e| panic!("{e}\n{}", out.toml));
    assert!(!p.has_changes(), "export doesn't round-trip: {:?}\n{}", p.ops, out.toml);
}

#[tokio::test]
async fn recorded_fixtures_round_trip() {
    assert_round_trip(fixtures::lab()).await;
    assert_round_trip(fixtures::rich()).await;
    assert_round_trip(fixtures::empty(Id(1000), "Empty")).await;
}

fn arb_perms() -> impl Strategy<Value = PermissionSet> {
    // Known and unknown bits alike must survive.
    any::<u64>().prop_map(|b| PermissionSet::from_bits(b & ((1 << 53) - 1)))
}

prop_compose! {
    fn arb_snapshot()(
        colors in proptest::collection::vec(0u32..0xFF_FFFF, 3),
        flags in proptest::collection::vec(any::<bool>(), 8),
        perms in proptest::collection::vec(arb_perms(), 6),
        topics in proptest::collection::vec(proptest::option::of("[a-z ]{0,30}"), 3),
        positions in proptest::collection::vec(0i32..20, 6),
        slowmode in 0u32..21_600,
        member_overwrite in any::<bool>(),
    ) -> Snapshot {
        let mut s = fixtures::rich();
        for (r, c) in s.roles.iter_mut().filter(|r| !r.managed && r.id != fixtures::GUILD).zip(&colors) {
            r.color = *c;
        }
        for (i, r) in s.roles.iter_mut().filter(|r| !r.managed).enumerate() {
            r.permissions = perms[i];
            r.hoist = flags[i];
            r.mentionable = flags[i + 3];
        }
        let known = s.channels.iter().filter(|c| matches!(c.kind, guildsmith_core::snapshot::LiveChannelKind::Known(k) if k == guildsmith_core::spec::ChannelKind::Text)).map(|c| c.id).collect::<Vec<_>>();
        for (id, t) in known.iter().zip(&topics) {
            let c = s.channels.iter_mut().find(|c| c.id == *id).unwrap();
            c.topic = t.clone().filter(|t| !t.is_empty());
            c.slowmode = slowmode;
            c.nsfw = flags[6];
        }
        for (c, p) in s.channels.iter_mut().zip(&positions) {
            c.position = *p;
        }
        if member_overwrite {
            let general = s.channels.iter_mut().find(|c| c.id == fixtures::GENERAL).unwrap();
            general.overwrites.push(LiveOverwrite { id: fixtures::ALEX, kind: OverwriteKind::Member, allow: perms[4], deny: perms[5].difference(perms[4]) });
        }
        s
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn generated_snapshots_round_trip(s in arb_snapshot()) {
        // Through text, without the async app: export → write → parse → diff.
        let (spec, notes) = guildsmith_core::export::to_spec(&s);
        let header = guildsmith_core::spec::writer::Header {
            version: "0.1.0", guild_name: &s.guild.name, guild_id: "1000", date: "2026-10-19",
        };
        let text = guildsmith_core::spec::writer::write(&spec, &header, &notes);
        let parsed = parse(&text).map_err(|e| TestCaseError::fail(format!("{e:?}\n{text}")))?;
        let p = diff(&parsed, &s).map_err(|e| TestCaseError::fail(format!("{e}\n{text}")))?;
        prop_assert!(p.ops.is_empty(), "{:?}\n{}", p.ops.iter().map(|o| &o.summary).collect::<Vec<_>>(), text);
    }
}
