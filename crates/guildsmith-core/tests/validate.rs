//! T040: one test per validation rule in data-model §1.
// Fixture text is built with format! for readability.
#![allow(clippy::format_push_string, clippy::format_collect)]

use guildsmith_core::validate::{Severity, validate_text};

/// Validates `text` and returns `(severity, message, "line:col")` for each problem.
fn problems(text: &str) -> Vec<(Severity, String, String)> {
    let report = validate_text(text);
    report
        .problems
        .iter()
        .map(|p| (p.severity, p.message.clone(), p.location.map(|l| l.to_string()).unwrap_or_default()))
        .collect()
}

fn errors(text: &str) -> Vec<String> {
    problems(text).into_iter().filter(|p| p.0 == Severity::Error).map(|p| p.1).collect()
}

fn assert_no_errors(text: &str) {
    assert_eq!(errors(text), Vec::<String>::new());
}

fn has_error(text: &str, needle: &str) -> bool {
    let errs = errors(text);
    errs.iter().any(|e| e.contains(needle)) || panic!("expected an error containing {needle:?}, got {errs:?}")
}

#[test]
fn valid_spec_has_no_problems() {
    let text = r##"
[guild]
name = "Lab"

[[roles]]
name = "Moderator"
color = "#3BA55D"
permissions = ["kick_members"]

[[roles]]
name = "@everyone"
permissions = ["view_channel"]

[[categories]]
name = "Community"

  [[categories.channels]]
  name = "general"
  kind = "text"
  topic = "hi"

  [[categories.channels]]
  name = "mod-log"
  kind = "text"
  overwrites = [
    { target = "@everyone", deny = ["view_channel"] },
    { target = "@Moderator", allow = ["view_channel"] },
  ]

[[channels]]
name = "Lobby"
kind = "voice"
user_limit = 10
"##;
    let report = validate_text(text);
    assert!(report.spec.is_some());
    assert_eq!(problems(text), vec![]);
}

#[test]
fn role_count_limit() {
    let roles: String = (0..251).map(|i| format!("[[roles]]\nname = \"r{i}\"\n")).collect();
    assert!(has_error(&roles, "at most 250 roles"));
}

#[test]
fn channel_count_limits() {
    let mut text = String::new();
    for c in 0..11 {
        text.push_str(&format!("[[categories]]\nname = \"c{c}\"\n"));
        for i in 0..50 {
            text.push_str(&format!("[[categories.channels]]\nname = \"ch{i}\"\nkind = \"text\"\n"));
        }
    }
    // 11 categories + 550 channels = 561 > 500.
    assert!(has_error(&text, "at most 500 channels"));
    let mut one = String::from("[[categories]]\nname = \"big\"\n");
    for i in 0..51 {
        one.push_str(&format!("[[categories.channels]]\nname = \"ch{i}\"\nkind = \"text\"\n"));
    }
    assert!(has_error(&one, "at most 50 channels"));
}

#[test]
fn name_lengths() {
    assert!(has_error("[[roles]]\nname = \"\"\n", "1–100 characters"));
    let long = "x".repeat(101);
    assert!(has_error(&format!("[[roles]]\nname = \"{long}\"\n"), "1–100 characters"));
    assert!(has_error(&format!("[[channels]]\nname = \"{long}\"\nkind = \"text\"\n"), "1–100 characters"));
}

#[test]
fn topic_rules() {
    let t = "a".repeat(1025);
    assert!(has_error(&format!("[[channels]]\nname = \"a\"\nkind = \"text\"\ntopic = \"{t}\"\n"), "0–1024"));
    // Forums allow up to 4096.
    assert_no_errors(&format!("[[channels]]\nname = \"a\"\nkind = \"forum\"\ntopic = \"{t}\"\n"));
    let t = "a".repeat(4097);
    assert!(has_error(&format!("[[channels]]\nname = \"a\"\nkind = \"forum\"\ntopic = \"{t}\"\n"), "0–4096"));
    assert!(has_error(
        "[[channels]]\nname = \"v\"\nkind = \"voice\"\ntopic = \"x\"\n",
        "topic is not allowed on voice"
    ));
}

#[test]
fn slowmode_rules() {
    assert!(has_error("[[channels]]\nname = \"a\"\nkind = \"text\"\nslowmode = 21601\n", "0–21600"));
    assert!(has_error(
        "[[channels]]\nname = \"a\"\nkind = \"announcement\"\nslowmode = 5\n",
        "slowmode is not allowed on announcement"
    ));
}

#[test]
fn voice_rules() {
    assert!(has_error("[[channels]]\nname = \"v\"\nkind = \"voice\"\nbitrate = 7999\n", "≥ 8000"));
    assert!(has_error("[[channels]]\nname = \"v\"\nkind = \"voice\"\nuser_limit = 100\n", "0–99"));
    assert_no_errors("[[channels]]\nname = \"s\"\nkind = \"stage\"\nuser_limit = 10000\n");
    assert!(has_error("[[channels]]\nname = \"s\"\nkind = \"stage\"\nuser_limit = 10001\n", "≤ 10000"));
    assert!(has_error(
        "[[channels]]\nname = \"a\"\nkind = \"text\"\nbitrate = 64000\n",
        "bitrate is only allowed on voice and stage"
    ));
}

#[test]
fn tag_rules() {
    let tags: String = (0..21).map(|i| format!("{{ name = \"t{i}\" }}, ")).collect();
    assert!(has_error(&format!("[[channels]]\nname = \"f\"\nkind = \"forum\"\ntags = [{tags}]\n"), "at most 20 tags"));
    assert!(has_error(
        "[[channels]]\nname = \"f\"\nkind = \"forum\"\ntags = [{ name = \"aaaaaaaaaaaaaaaaaaaaa\" }]\n",
        "≤ 20 characters"
    ));
    assert!(has_error(
        "[[channels]]\nname = \"a\"\nkind = \"text\"\ntags = [{ name = \"x\" }]\n",
        "tags are only allowed on forum and media"
    ));
}

#[test]
fn overwrite_rules() {
    let base = "[[roles]]\nname = \"Mod\"\n[[channels]]\nname = \"a\"\nkind = \"text\"\n";
    assert!(has_error(
        &format!(
            "{base}overwrites = [{{ target = \"@Mod\", allow = [\"view_channel\"], deny = [\"view_channel\"] }}]\n"
        ),
        "both allow and deny"
    ));
    assert!(has_error(
        &format!(
            "{base}overwrites = [{{ target = \"@Mod\", allow = [\"view_channel\"] }}, {{ target = \"@Mod\", deny = [\"send_messages\"] }}]\n"
        ),
        "appears more than once"
    ));
    assert!(has_error(
        &format!("{base}overwrites = [{{ target = \"@Modd\", allow = [\"view_channel\"] }}]\n"),
        "unknown role @Modd"
    ));
    // role:<id> and member:<id> can't be checked offline and are accepted.
    assert_no_errors(&format!("{base}overwrites = [{{ target = \"role:123\" }}, {{ target = \"member:5\" }}]\n"));
}

#[test]
fn sync_permissions_rules() {
    let text = "[[categories]]\nname = \"c\"\n[[categories.channels]]\nname = \"a\"\nkind = \"text\"\nsync_permissions = true\noverwrites = [{ target = \"@everyone\", deny = [\"view_channel\"] }]\n";
    assert!(has_error(text, "sync_permissions = true needs empty overwrites"));
    assert!(has_error(
        "[[channels]]\nname = \"a\"\nkind = \"text\"\nsync_permissions = true\n",
        "applies only to channels in a category"
    ));
}

#[test]
fn settings_need_manage_settings() {
    // 002 lifts v1's rejection: `manage_settings = true` turns `[guild.settings]` on.
    assert_no_errors("[guild]\nmanage_settings = true\n\n[guild.settings]\ndescription = \"x\"\n");
    assert!(has_error("[guild.settings]\ndescription = \"x\"\n", "manage_settings = true"));
}

#[test]
fn unknown_keys_suggest_the_closest_key() {
    let report = validate_text("[[roles]]\nnmae = \"x\"\n");
    let p = &report.problems[0];
    assert_eq!(p.severity, Severity::Error);
    assert!(p.message.contains("unknown field `nmae`"), "{}", p.message);
    assert_eq!(p.hint.as_deref(), Some("did you mean `name`?"));
    let report = validate_text("[[roles]]\nname = \"x\"\npermissions = [\"view_channels\"]\n");
    assert_eq!(report.problems[0].hint.as_deref(), Some("did you mean `view_channel`?"));
}

#[test]
fn duplicate_names_are_errors() {
    assert!(has_error("[[roles]]\nname = \"A\"\n[[roles]]\nname = \"A\"\n", "duplicate role name"));
    assert!(has_error(
        "[[channels]]\nname = \"a\"\nkind = \"text\"\n[[channels]]\nname = \"a\"\nkind = \"voice\"\n",
        "duplicate channel name"
    ));
}

#[test]
fn everyone_entry_only_sets_permissions() {
    assert!(has_error("[[roles]]\nname = \"@everyone\"\nhoist = true\n", "@everyone can only set permissions"));
}

#[test]
fn text_like_names_warn_when_discord_would_rewrite_them() {
    let p = problems("[[channels]]\nname = \"General Chat\"\nkind = \"text\"\n");
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, Severity::Warning);
    assert!(p[0].1.contains("general-chat"), "{}", p[0].1);
    // Voice channel names keep their case.
    assert_eq!(problems("[[channels]]\nname = \"Lobby\"\nkind = \"voice\"\n"), vec![]);
}

#[test]
fn every_problem_reports_line_and_column() {
    let text = "[guild]\nname = \"x\"\n\n[[roles]]\nname = \"A\"\n\n[[channels]]\nname = \"v\"\nkind = \"voice\"\nuser_limit = 100\n";
    let p = problems(text);
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].2, "10:14", "points at the user_limit value");
    let p = problems("[[roles]]\nnmae = \"x\"\n");
    assert_eq!(p[0].2, "2:1");
}
