//! T053: the plan hash (research R4).

mod common;

use common::lab;
use guildsmith_core::hash::{is_plan_hash, plan_hash, spec_digest};
use guildsmith_core::spec::parse;
use guildsmith_core::{Id, diff};

const SPEC: &str = "[[roles]]\nname = \"Staff\"\n[[roles]]\nname = \"Moderator\"\nhoist = true\n";

#[test]
fn format_is_gsp_and_32_hex() {
    let p = diff(&parse(SPEC).unwrap(), &lab()).unwrap();
    assert!(p.hash.starts_with("gsp_"), "{}", p.hash);
    assert_eq!(p.hash.len(), 4 + 32);
    assert!(is_plan_hash(&p.hash));
    assert!(!is_plan_hash("gsp_123") && !is_plan_hash("made-up"));
}

#[test]
fn hash_covers_ops_fingerprint_spec_and_engine() {
    let spec = parse(SPEC).unwrap();
    let s = lab();
    let p = diff(&spec, &s).unwrap();
    assert_eq!(p.hash, plan_hash(&p.ops, &s.fingerprint(), &spec));
    assert_eq!(p.snapshot_fingerprint, s.fingerprint());

    // Formatting and comments don't change the spec digest; content does.
    let reformatted = parse(&format!("# a comment\n{SPEC}\n\n")).unwrap();
    assert_eq!(spec_digest(&spec), spec_digest(&reformatted));
    assert_eq!(diff(&reformatted, &s).unwrap().hash, p.hash);
    let other = parse(&SPEC.replace("hoist = true", "hoist = false")).unwrap();
    assert_ne!(spec_digest(&spec), spec_digest(&other));
}

#[test]
fn any_server_change_changes_the_hash() {
    let spec = parse(SPEC).unwrap();
    let before = diff(&spec, &lab()).unwrap();

    // A change the plan doesn't even touch: renaming an unrelated channel.
    let mut s = lab();
    s.channels.iter_mut().find(|c| c.id == Id(103)).unwrap().name = "Lounge".to_owned();
    let after = diff(&spec, &s).unwrap();
    assert_eq!(before.ops, after.ops, "same ops");
    assert_ne!(before.hash, after.hash, "different server, different hash");

    // Same server listed in another order: same fingerprint, same hash.
    let mut shuffled = lab();
    shuffled.roles.reverse();
    shuffled.channels.reverse();
    assert_eq!(diff(&spec, &shuffled).unwrap().hash, before.hash);
}
