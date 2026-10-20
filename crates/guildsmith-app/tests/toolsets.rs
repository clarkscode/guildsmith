//! T006: the toolset registry (data-model §1, research E1, FR-003–003c).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use guildsmith_app::App;
use guildsmith_app::config::{Config, ConfigError, Dirs, LoadOptions, MapEnv, NoStore, Os, load};
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::RefusalKind;
use guildsmith_app::ports::Clock;
use guildsmith_app::toolsets::{EnabledBy, Toolset, ToolsetState, parse_blocked, parse_toolsets};

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_000_000)
    }
}

fn config(env: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let env = MapEnv::new(env);
    let files: HashMap<PathBuf, String> = HashMap::new();
    let read = |p: &Path| files.get(p).cloned().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound));
    load(&LoadOptions {
        env: &env,
        config_path: None,
        dirs: Dirs { os: Os::Linux, home: Some("/home/u".into()), appdata: None },
        store: &NoStore,
        read_file: &read,
    })
}

#[test]
fn all_registers_every_group_and_default_only_core() {
    let all = config(&[("GUILDSMITH_TOOLSETS", "all")]).unwrap().policy();
    assert_eq!(all.toolsets, Toolset::ALL.iter().copied().collect::<BTreeSet<_>>());
    assert_eq!(Toolset::ALL.len(), 13);
    let default = config(&[("GUILDSMITH_TOOLSETS", "default")]).unwrap().policy();
    assert_eq!(default.toolsets, BTreeSet::from([Toolset::Core]));
    assert_eq!(config(&[]).unwrap().policy().toolsets, BTreeSet::from([Toolset::Core]));
    assert_eq!(parse_toolsets("polls, raw").unwrap(), BTreeSet::from([Toolset::Core, Toolset::Polls, Toolset::Raw]));
}

#[test]
fn an_unknown_name_is_a_startup_error_listing_valid_names() {
    let Err(e) = config(&[("GUILDSMITH_TOOLSETS", "core,gateway")]) else { panic!("expected an error") };
    let msg = e.to_string();
    for name in ["gateway", "core", "moderation", "messages", "settings", "raw", "all"] {
        assert!(msg.contains(name), "{name} missing from: {msg}");
    }
    assert!(config(&[("GUILDSMITH_TOOLSETS_BLOCK", "nope")]).is_err());
    assert!(parse_blocked("core").is_err(), "core can't be blocked");
}

#[test]
fn the_block_list_wins_over_the_enabled_list() {
    let cfg = config(&[("GUILDSMITH_TOOLSETS", "all"), ("GUILDSMITH_TOOLSETS_BLOCK", "raw,webhooks")]).unwrap();
    assert!(cfg.warnings.iter().any(|w| w.contains("raw") && w.contains("block wins")), "{:?}", cfg.warnings);
    let policy = cfg.policy();
    assert!(!policy.toolsets.contains(&Toolset::Raw));
    assert!(!policy.toolsets.contains(&Toolset::Webhooks));
    assert!(policy.toolsets.contains(&Toolset::Polls));
    assert_eq!(policy.blocked, BTreeSet::from([Toolset::Raw, Toolset::Webhooks]));

    let state = ToolsetState::new(&policy.toolsets, &policy.blocked, &Fixed);
    let r = state.enable(Toolset::Raw, &Fixed).unwrap_err();
    assert_eq!(r.kind, RefusalKind::Blocked);
    assert!(r.reason.contains("raw"), "{}", r.reason);
    assert!(!state.is_unlocked(Toolset::Raw));
    // Even a state built from an enabled+blocked list never registers the blocked group.
    let state = ToolsetState::new(&Toolset::ALL.iter().copied().collect(), &BTreeSet::from([Toolset::Raw]), &Fixed);
    assert!(!state.registered().contains(&Toolset::Raw));
    assert_eq!(parse_blocked("all").unwrap().len(), 12);
}

#[test]
fn the_agent_can_only_grow_the_unlocked_set() {
    let policy = guildsmith_app::policy::Policy {
        allowlist: Some(BTreeSet::from([fixtures::GUILD])),
        read_only: true,
        blocked: BTreeSet::from([Toolset::Raw]),
        ..guildsmith_app::policy::Policy::default()
    };
    let app = Arc::new(App::with_clock(fixtures::lab_fake(), policy.clone(), Arc::new(Fixed)));
    for t in Toolset::ALL {
        let _ = app.toolsets.enable(*t, &Fixed);
    }
    // Nothing the agent can call changes the operator's rules.
    assert_eq!(app.policy, policy);
    assert_eq!(app.toolsets.blocked(), &BTreeSet::from([Toolset::Raw]));
    assert_eq!(app.toolsets.registered(), &BTreeSet::from([Toolset::Core]));
    assert_eq!(app.toolsets.unlocked().len(), 12, "every group but the blocked one");
}

#[test]
fn every_enablement_is_logged_with_time_group_and_who() {
    let registered = BTreeSet::from([Toolset::Core, Toolset::Moderation]);
    let state = ToolsetState::new(&registered, &BTreeSet::new(), &Fixed);
    let first = state.enablements();
    assert_eq!(first.len(), 1, "core isn't logged; moderation is, by the operator");
    assert_eq!((first[0].toolset, first[0].by), (Toolset::Moderation, EnabledBy::Operator));

    let e = state.enable(Toolset::Polls, &Fixed).unwrap().expect("newly enabled");
    assert_eq!(e.toolset, Toolset::Polls);
    assert_eq!(e.by, EnabledBy::Agent);
    assert_eq!(e.at, "2026-10-14T17:46:40Z");
    assert!(state.is_unlocked(Toolset::Polls));
    assert!(state.enable(Toolset::Polls, &Fixed).unwrap().is_none(), "enabling twice logs once");
    assert!(state.enable(Toolset::Moderation, &Fixed).unwrap().is_none(), "already registered");
    assert_eq!(state.enablements().len(), 2);
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(json, serde_json::json!({"at": "2026-10-14T17:46:40Z", "toolset": "polls", "by": "agent"}));
}

#[test]
fn every_tool_belongs_to_exactly_one_group_and_needs_known_permissions() {
    let mut seen = BTreeSet::new();
    for t in Toolset::ALL {
        assert!(t.tools().len() <= if *t == Toolset::Core { 25 } else { 15 }, "{t} is over its tool budget");
        for tool in t.tools() {
            assert!(seen.insert(*tool), "{tool} is in two groups");
            assert_eq!(Toolset::of_tool(tool), Some(*t));
            assert!(guildsmith_app::preflight::tool_needs(tool).is_some(), "{tool} has no permission entry");
        }
    }
    assert_eq!(seen.len(), 84);
    assert_eq!(Toolset::Core.tools().len(), 23);
    assert!(Toolset::Moderation.required_permissions().contains(guildsmith_core::Permission::BanMembers));
    assert_eq!(Toolset::Messages.required_intents(), &["MESSAGE_CONTENT"]);
}
