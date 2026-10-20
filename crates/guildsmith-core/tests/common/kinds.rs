//! Fixtures for the kinds beyond roles and channels: valid files, a spec with one item of
//! every kind, and an empty live view of each kind.

use std::collections::BTreeMap;

use guildsmith_core::asset::{self, Asset, FileType};
use guildsmith_core::live::{GuildSettings, LiveExtras, Onboarding, WelcomeScreen};
use guildsmith_core::planner::SpecAssets;
use guildsmith_core::spec::{GuildSpec, PathSeg, SpecPath};
use guildsmith_core::validate::{Severity, validate_text};

/// 2026-10-18T00:00:00Z.
pub const NOW: i64 = 1_792_281_600;

pub fn png(seed: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend([0, 0, 0, 13]);
    b.extend(b"IHDR");
    b.extend([seed; 13 + 4]);
    b.extend([0, 0, 0, 0]);
    b.extend(b"IDAT");
    b.resize(64, seed);
    b
}

/// MPEG-1 Layer III at 128 kbps / 44.1 kHz: 417 bytes and 1152 samples per frame.
pub fn mp3(frames: usize) -> Vec<u8> {
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
    frame.resize(417, 0);
    frame.repeat(frames)
}

pub fn png_uri(seed: u8) -> String {
    asset::data_uri(FileType::Png, &png(seed))
}

pub fn mp3_uri(frames: usize) -> String {
    asset::data_uri(FileType::Mp3, &mp3(frames))
}

/// One item of every kind (inline files, so no I/O).
#[allow(clippy::too_many_lines)] // one TOML document
pub fn full_text() -> String {
    format!(
        r##"
[guild]
name = "Guildsmith Lab"
manage_settings = true

[guild.settings]
description = "A lab for guildsmith"
icon = {{ data = "{icon}" }}
rules_channel = "#rules"
public_updates_channel = "#mod-log"
system_channel = "#general"
verification_level = "medium"
explicit_content_filter = "all_members"
afk_timeout = 300
features = {{ community = true }}

[[roles]]
name = "Moderator"

[[roles]]
name = "Member"

[[categories]]
name = "Community"

  [[categories.channels]]
  name = "general"
  kind = "text"

  [[categories.channels]]
  name = "rules"
  kind = "text"

  [[categories.channels]]
  name = "announcements"
  kind = "text"

  [[categories.channels]]
  name = "help"
  kind = "text"

  [[categories.channels]]
  name = "off-topic"
  kind = "text"

  [[categories.channels]]
  name = "events"
  kind = "text"

  [[categories.channels]]
  name = "welcome"
  kind = "text"

  [[categories.channels]]
  name = "mod-log"
  kind = "text"

[[channels]]
name = "Lobby"
kind = "voice"

[[automod]]
name = "No invite links"
trigger = {{ keyword = {{ regex = ['discord\.gg/\w+'] }} }}
actions = [{{ block = {{ message = "Invites aren't allowed here." }} }}, {{ alert = {{ channel = "#mod-log" }} }}]
exempt_roles = ["@Moderator"]

[onboarding]
enabled = true
mode = "advanced"
default_channels = ["#welcome", "#rules", "#general", "#announcements", "#help", "#off-topic", "#events"]

  [[onboarding.prompts]]
  title = "What brings you here?"
  options = [{{ title = "Just looking", roles = ["@Member"] }}]

[welcome_screen]
enabled = true
description = "Welcome!"
channels = [{{ channel = "#rules", description = "Read the rules" }}]

[[emoji]]
name = "guildsmith"
image = {{ data = "{emoji}" }}
roles = ["@Member"]

[[stickers]]
name = "Hammer"
tags = "hammer"
image = {{ data = "{sticker}" }}

[[sounds]]
name = "ding"
sound = {{ data = "{sound}" }}
volume = 0.5

[[webhooks]]
name = "CI"
channel = "#general"
avatar = {{ data = "{avatar}" }}

[[invites]]
channel = "#welcome"
max_age = 0
roles = ["@Member"]

[[events]]
name = "Game night"
start = 2026-10-23T20:00:00-07:00
end = 2026-10-23T22:00:00-07:00
location = {{ voice = "Lobby" }}
recurrence = {{ frequency = "weekly", weekday = "friday" }}

[[messages]]
channel = "#rules"
pinned = true
content = """
1. Be kind.
2. No spam.
"""
"##,
        icon = png_uri(1),
        emoji = png_uri(2),
        sticker = png_uri(3),
        sound = mp3_uri(4),
        avatar = png_uri(5),
    )
}

pub fn parse(text: &str) -> GuildSpec {
    let report = validate_text(text);
    let errors: Vec<_> = report.problems.iter().filter(|p| p.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    report.spec.expect("parses")
}

/// Every live kind read, and empty; settings as Discord makes a new server.
pub fn empty() -> LiveExtras {
    LiveExtras {
        settings: Some(GuildSettings {
            name: "Guildsmith Lab".to_owned(),
            preferred_locale: "en-US".to_owned(),
            ..GuildSettings::default()
        }),
        automod: Some(Vec::new()),
        onboarding: Some(Onboarding::default()),
        welcome_screen: Some(WelcomeScreen::default()),
        expressions: Some(Vec::new()),
        webhooks: Some(Vec::new()),
        invites: Some(Vec::new()),
        events: Some(Vec::new()),
        messages: Some(BTreeMap::new()),
        asset_hashes: BTreeMap::new(),
        now: NOW,
    }
}

fn walk(path: &SpecPath, v: &serde_json::Value, out: &mut SpecAssets) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(serde_json::Value::String(d)) = m.get("data")
                && m.len() == 1
            {
                let bytes = asset::decode_data(d).expect("valid data");
                out.insert(path.to_string(), asset::hash(&bytes));
                return;
            }
            for (k, x) in m {
                walk(&path.clone().key(k), x, out);
            }
        }
        serde_json::Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                walk(&path.clone().index(i), x, out);
            }
        }
        _ => {}
    }
}

/// The hash of every inline file in `spec`, by spec path (what the app reads).
pub fn assets(spec: &GuildSpec) -> SpecAssets {
    let mut out = SpecAssets::new();
    walk(&SpecPath::root(), &serde_json::to_value(spec).unwrap(), &mut out);
    out
}

/// Keeps `Asset` and `PathSeg` in use for the tests that need them.
pub fn _types(_: Asset, _: PathSeg) {}
