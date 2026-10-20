//! T088: validating the kinds beyond roles and channels: secrets are rejected anywhere (E8),
//! sections the spec doesn't manage are unknown keys (FR-043), times need an offset,
//! `components_v2` excludes content and embeds, external events need an end, and references
//! that resolve nowhere stop the plan.

mod common;

use common::kinds;
use guildsmith_core::diff_with;
use guildsmith_core::validate::{Severity, validate_text};

fn errors(text: &str) -> Vec<String> {
    validate_text(text)
        .problems
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .map(|p| format!("{}: {}", p.path, p.message))
        .collect()
}

fn has_error(text: &str, needle: &str) {
    let e = errors(text);
    assert!(e.iter().any(|m| m.contains(needle)), "expected an error with {needle:?}, got {e:#?}");
}

#[test]
fn the_full_example_is_valid() {
    assert_eq!(errors(&kinds::full_text()), Vec::<String>::new());
}

#[test]
fn webhook_urls_and_tokens_are_rejected_anywhere() {
    let url = "https://discord.com/api/webhooks/123456789012345678/AbCdEf0123456789_-AbCdEf0123456789_-AbCdEf0123456789_-AbCdEf01";
    for text in [
        format!("[[messages]]\nchannel = \"#rules\"\ncontent = \"{url}\"\n"),
        format!("[guild]\nname = \"{url}\"\n"),
        format!(
            "[[messages]]\nchannel = \"#rules\"\nembeds = [{{ description = \"x\", url = \"{url}\" }}]\n"
        ),
        "[[messages]]\nchannel = \"#rules\"\ncontent = \"AbCdEf0123456789_-AbCdEf0123456789_-AbCdEf0123456789_-AbCdEf01xy\"\n"
            .to_owned(),
    ] {
        has_error(&text, "secrets never go in the spec file");
    }
    // A webhook has no field for its URL or token at all.
    has_error("[[webhooks]]\nname = \"CI\"\nchannel = \"#general\"\nurl = \"x\"\n", "unknown field `url`");
    has_error("[[webhooks]]\nname = \"CI\"\nchannel = \"#general\"\ntoken = \"x\"\n", "unknown field `token`");
}

#[test]
fn unmanaged_sections_are_unknown_keys() {
    for section in ["bans", "members", "member_roles", "threads", "polls", "voice_states", "stage_instances"] {
        has_error(&format!("[[{section}]]\nname = \"x\"\n"), "unknown field");
    }
}

#[test]
fn times_need_an_offset() {
    let event = |start: &str| {
        format!(
            "[[events]]\nname = \"x\"\nstart = {start}\nend = 2026-10-24T05:00:00Z\nlocation = {{ voice = \"Lobby\" }}\n"
        )
    };
    has_error(&event("2026-10-23T20:00:00"), "no UTC offset");
    has_error(&event("2026-10-23"), "no UTC offset");
    assert_eq!(errors(&event("2026-10-23T20:00:00-07:00")), Vec::<String>::new());
    assert_eq!(errors(&event("\"2026-10-23T20:00:00Z\"")), Vec::<String>::new(), "an RFC 3339 string works too");
}

#[test]
fn external_events_need_an_end_after_the_start() {
    has_error(
        "[[events]]\nname = \"Meetup\"\nstart = 2026-10-23T20:00:00Z\nlocation = { external = { place = \"Park\" } }\n",
        "needs an `end`",
    );
    has_error(
        "[[events]]\nname = \"x\"\nstart = 2026-10-23T20:00:00Z\nend = 2026-10-23T19:00:00Z\nlocation = { voice = \"Lobby\" }\n",
        "after `start`",
    );
}

#[test]
fn components_v2_excludes_content_and_embeds() {
    has_error(
        "[[messages]]\nchannel = \"#rules\"\ncontent = \"hi\"\ncomponents_v2 = [{ type = 10, content = \"hi\" }]\n",
        "can't also have content or embeds",
    );
    has_error("[[messages]]\nchannel = \"#rules\"\n", "needs content, embeds or components_v2");
}

#[test]
fn kind_limits_are_checked() {
    has_error("[[emoji]]\nname = \"a b\"\nimage = { url = \"https://x/y.png\" }\n", "emoji names");
    has_error("[[webhooks]]\nname = \"Clyde\"\nchannel = \"#general\"\n", "clyde");
    has_error("[[invites]]\nchannel = \"#general\"\nmax_age = 999999\n", "max_age");
    has_error("[[sounds]]\nname = \"ding\"\nsound = { url = \"https://x/y.mp3\" }\nvolume = 2.0\n", "volume");
    has_error(&format!("[[emoji]]\nname = \"big\"\nimage = {{ data = \"{}\" }}\n", kinds::mp3_uri(2)), "emoji");
    has_error("[[invites]]\nchannel = \"#general\"\n\n[[invites]]\nchannel = \"#general\"\n", "can't be told apart");
    has_error("[[onboarding.prompts]]\ntitle = \"x\"\noptions = []\n", "missing field `enabled`");
}

#[test]
fn unresolved_references_stop_the_plan() {
    let text = "[[webhooks]]\nname = \"CI\"\nchannel = \"#nowhere\"\n\n[[emoji]]\nname = \"x1\"\nimage = { url = \"https://x/y.png\" }\nroles = [\"@Nobody\"]\n";
    let spec = kinds::parse(text);
    let mut assets = kinds::assets(&spec);
    assets.insert("emoji[0].image".to_owned(), "h".to_owned());
    let err = diff_with(&spec, &common::lab(), &kinds::empty(), &assets).unwrap_err();
    let all = err.problems.join("\n");
    assert!(all.contains("`#nowhere` is not a channel"), "{all}");
    assert!(all.contains("`@Nobody` is not a role"), "{all}");
}
