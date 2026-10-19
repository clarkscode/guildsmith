//! T016: allowlist, read-only mode and toolset parsing.

use std::collections::BTreeSet;

use guildsmith_app::policy::{Policy, RefusalKind, Toolset, parse_allowlist, parse_toolsets};
use guildsmith_core::Id;

#[test]
fn allowlist_refuses_other_servers() {
    let allow = parse_allowlist("123, 456").unwrap();
    let policy = Policy { allowlist: Some(allow), read_only: false, toolsets: BTreeSet::from([Toolset::Core]) };
    assert!(policy.ensure_allowed(Id(123)).is_ok());
    assert!(policy.ensure_allowed(Id(456)).is_ok());
    let r = policy.ensure_allowed(Id(789)).unwrap_err();
    assert_eq!(r.kind, RefusalKind::NotAllowed);
    assert!(r.reason.contains("789"));
    assert!(r.hint.as_deref().unwrap_or("").contains("GUILDSMITH_GUILDS"));
}

#[test]
fn no_allowlist_allows_everything() {
    let policy = Policy { allowlist: None, read_only: false, toolsets: BTreeSet::from([Toolset::Core]) };
    assert!(policy.ensure_allowed(Id(1)).is_ok());
    assert!(policy.allowlist_warning().is_some());
}

#[test]
fn bad_allowlist_entries_are_errors() {
    assert!(parse_allowlist("123,abc").is_err());
    assert_eq!(parse_allowlist("").unwrap(), BTreeSet::new());
}

#[test]
fn read_only_refuses_writes() {
    let policy = Policy { allowlist: None, read_only: true, toolsets: BTreeSet::from([Toolset::Core]) };
    let r = policy.ensure_writable().unwrap_err();
    assert_eq!(r.kind, RefusalKind::ReadOnly);
    let policy = Policy { read_only: false, ..policy };
    assert!(policy.ensure_writable().is_ok());
}

#[test]
fn toolsets_parse() {
    assert_eq!(parse_toolsets("core,moderation").unwrap(), BTreeSet::from([Toolset::Core, Toolset::Moderation]));
    // Core is always on; `default` means core only.
    assert_eq!(parse_toolsets("moderation").unwrap(), BTreeSet::from([Toolset::Core, Toolset::Moderation]));
    assert_eq!(parse_toolsets("default").unwrap(), BTreeSet::from([Toolset::Core]));
    assert_eq!(parse_toolsets(" ").unwrap(), BTreeSet::from([Toolset::Core]));
}

#[test]
fn unknown_toolset_lists_valid_names() {
    let err = parse_toolsets("core,events").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("events"), "{msg}");
    assert!(msg.contains("core") && msg.contains("moderation"), "{msg}");
}
