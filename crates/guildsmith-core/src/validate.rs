//! Offline validation of spec files (FR-006, data-model §1). No network access.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::permissions::Permission;
use crate::spec::{
    self, CategorySpec, ChannelKind, ChannelSpec, GuildSpec, Location, OverwriteSpec, SpecPath, SpecSource, Target,
};
use crate::suggest;

pub const MAX_ROLES: usize = 250;
pub const MAX_CHANNELS: usize = 500;
pub const MAX_PER_CATEGORY: usize = 50;
pub const MAX_TAGS: usize = 20;

/// How serious a problem is. Errors block planning; warnings don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// One validation problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Problem {
    pub severity: Severity,
    /// Where in the document, e.g. `channels[0].user_limit`.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The result of validating spec text.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// `None` when the text didn't parse.
    pub spec: Option<GuildSpec>,
    pub problems: Vec<Problem>,
}

impl Report {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.problems.iter().any(|p| p.severity == Severity::Error)
    }
}

struct Ctx {
    out: Vec<(Severity, SpecPath, String, Option<String>)>,
}

impl Ctx {
    fn error(&mut self, path: SpecPath, msg: impl Into<String>) {
        self.out.push((Severity::Error, path, msg.into(), None));
    }

    fn error_hint(&mut self, path: SpecPath, msg: impl Into<String>, hint: Option<String>) {
        self.out.push((Severity::Error, path, msg.into(), hint));
    }

    fn warn(&mut self, path: SpecPath, msg: impl Into<String>, hint: &str) {
        self.out.push((Severity::Warning, path, msg.into(), Some(hint.to_owned())));
    }
}

fn len(s: &str) -> usize {
    s.chars().count()
}

/// The name Discord stores for a text-like channel: lowercase, spaces become `-`.
#[must_use]
pub fn discord_text_name(name: &str) -> String {
    name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
}

fn check_overwrites(ctx: &mut Ctx, path: &SpecPath, overwrites: &[OverwriteSpec], role_names: &BTreeSet<&str>) {
    let mut seen = BTreeSet::new();
    for (i, o) in overwrites.iter().enumerate() {
        let p = path.clone().index(i);
        if !o.allow.intersection(o.deny).is_empty() {
            let both: Vec<String> = o.allow.intersection(o.deny).names();
            ctx.error(p.clone(), format!("{} is in both allow and deny", both.join(", ")));
        }
        if !seen.insert(o.target.clone()) {
            ctx.error(p.clone().key("target"), format!("target {} appears more than once in this channel", o.target));
        }
        if let Target::RoleName(name) = &o.target
            && !role_names.contains(name.as_str())
        {
            let hint =
                suggest::closest(name, role_names.iter().copied(), 1).first().map(|s| format!("did you mean `@{s}`?"));
            ctx.error_hint(p.key("target"), format!("unknown role @{name}"), hint);
        }
    }
}

fn check_text_fields(ctx: &mut Ctx, path: &SpecPath, ch: &ChannelSpec) {
    let k = ch.kind;
    if k.is_text_like() {
        let stored = discord_text_name(&ch.name);
        if stored != ch.name && !stored.is_empty() {
            ctx.warn(
                path.clone().key("name"),
                format!("Discord will store `{}` as `{stored}`", ch.name),
                "use the lowercase name so plans don't rename the channel every time",
            );
        }
    }
    if let Some(t) = &ch.topic {
        if k.is_voice_like() {
            ctx.error(path.clone().key("topic"), format!("topic is not allowed on {k} channels"));
        } else if k.is_forum_like() && len(t) > 4096 {
            ctx.error(path.clone().key("topic"), "topic must be 0–4096 characters on forum and media channels");
        } else if !k.is_forum_like() && len(t) > 1024 {
            ctx.error(path.clone().key("topic"), "topic must be 0–1024 characters");
        }
    }
    if let Some(s) = ch.slowmode {
        if k == ChannelKind::Announcement {
            ctx.error(path.clone().key("slowmode"), "slowmode is not allowed on announcement channels");
        } else if s > 21_600 {
            ctx.error(path.clone().key("slowmode"), "slowmode must be 0–21600 seconds");
        }
    }
}

fn check_voice_fields(ctx: &mut Ctx, path: &SpecPath, ch: &ChannelSpec) {
    let k = ch.kind;
    for (field, set) in [
        ("bitrate", ch.bitrate.is_some()),
        ("user_limit", ch.user_limit.is_some()),
        ("rtc_region", ch.rtc_region.is_some()),
    ] {
        if set && !k.is_voice_like() {
            ctx.error(path.clone().key(field), format!("{field} is only allowed on voice and stage channels"));
        }
    }
    if let Some(b) = ch.bitrate
        && k.is_voice_like()
        && b < 8000
    {
        ctx.error(path.clone().key("bitrate"), "bitrate must be ≥ 8000");
    }
    if let Some(u) = ch.user_limit {
        if k == ChannelKind::Voice && u > 99 {
            ctx.error(path.clone().key("user_limit"), "user_limit must be 0–99 on voice channels");
        } else if k == ChannelKind::Stage && u > 10_000 {
            ctx.error(path.clone().key("user_limit"), "user_limit must be ≤ 10000 on stage channels");
        }
    }
}

fn check_forum_fields(ctx: &mut Ctx, path: &SpecPath, ch: &ChannelSpec) {
    let k = ch.kind;
    if let Some(tags) = &ch.tags {
        if !k.is_forum_like() {
            ctx.error(path.clone().key("tags"), "tags are only allowed on forum and media channels");
        } else if tags.len() > MAX_TAGS {
            ctx.error(path.clone().key("tags"), "a forum can have at most 20 tags");
        }
        for (i, t) in tags.iter().enumerate() {
            if !(1..=20).contains(&len(&t.name)) {
                ctx.error(
                    path.clone().key("tags").index(i).key("name"),
                    "tag names must be 1–20 characters (≤ 20 characters)",
                );
            }
        }
    }
    if ch.default_reaction.is_some() && !k.is_forum_like() {
        ctx.error(path.clone().key("default_reaction"), "default_reaction is only allowed on forum and media channels");
    }
}

fn check_channel(ctx: &mut Ctx, path: &SpecPath, ch: &ChannelSpec, in_category: bool, role_names: &BTreeSet<&str>) {
    if !(1..=100).contains(&len(&ch.name)) {
        ctx.error(path.clone().key("name"), "channel names must be 1–100 characters");
    }
    check_text_fields(ctx, path, ch);
    check_voice_fields(ctx, path, ch);
    check_forum_fields(ctx, path, ch);
    if ch.sync_permissions {
        if !in_category {
            ctx.error(path.clone().key("sync_permissions"), "sync_permissions applies only to channels in a category");
        }
        if ch.overwrites.as_ref().is_some_and(|o| !o.is_empty()) {
            ctx.error(path.clone().key("overwrites"), "sync_permissions = true needs empty overwrites");
        }
    }
    if let Some(o) = &ch.overwrites {
        check_overwrites(ctx, &path.clone().key("overwrites"), o, role_names);
    }
}

fn check_unique<'a>(ctx: &mut Ctx, what: &str, items: impl Iterator<Item = (SpecPath, &'a str)>) {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (path, name) in items {
        if !seen.insert(name) {
            ctx.error(
                path.key("name"),
                format!("duplicate {what} name `{name}`; names must be unique so items can be matched"),
            );
        }
    }
}

fn check_category(ctx: &mut Ctx, i: usize, cat: &CategorySpec, role_names: &BTreeSet<&str>) {
    let path = SpecPath::root().key("categories").index(i);
    if !(1..=100).contains(&len(&cat.name)) {
        ctx.error(path.clone().key("name"), "category names must be 1–100 characters");
    }
    if cat.channels.len() > MAX_PER_CATEGORY {
        ctx.error(
            path.clone().key("channels"),
            format!("a category can hold at most 50 channels (has {})", cat.channels.len()),
        );
    }
    if let Some(o) = &cat.overwrites {
        check_overwrites(ctx, &path.clone().key("overwrites"), o, role_names);
    }
    check_unique(
        ctx,
        "channel",
        cat.channels.iter().enumerate().map(|(j, c)| (path.clone().key("channels").index(j), c.name.as_str())),
    );
    for (j, ch) in cat.channels.iter().enumerate() {
        check_channel(ctx, &path.clone().key("channels").index(j), ch, true, role_names);
    }
}

/// Validates a parsed spec. Problems carry paths but no locations.
#[must_use]
pub fn validate(spec: &GuildSpec) -> Vec<(Severity, SpecPath, String, Option<String>)> {
    let mut ctx = Ctx { out: Vec::new() };
    if spec.guild.manage_settings {
        ctx.error(
            SpecPath::root().key("guild").key("manage_settings"),
            "managing server settings is not supported in v1",
        );
    }
    let named: Vec<_> = spec.roles.iter().filter(|r| r.name != "@everyone").collect();
    if named.len() > MAX_ROLES {
        ctx.error(
            SpecPath::root().key("roles"),
            format!("a server can have at most 250 roles (spec lists {})", named.len()),
        );
    }
    let role_names: BTreeSet<&str> = named.iter().map(|r| r.name.as_str()).collect();
    for (i, r) in spec.roles.iter().enumerate() {
        let path = SpecPath::root().key("roles").index(i);
        if !(1..=100).contains(&len(&r.name)) {
            ctx.error(path.clone().key("name"), "role names must be 1–100 characters");
        }
        if r.name == "@everyone"
            && (r.color.is_some() || r.hoist.is_some() || r.mentionable.is_some() || r.position.is_some() || r.managed)
        {
            ctx.error(path, "@everyone can only set permissions");
        }
    }
    check_unique(
        &mut ctx,
        "role",
        spec.roles.iter().enumerate().map(|(i, r)| (SpecPath::root().key("roles").index(i), r.name.as_str())),
    );
    let channel_count = spec.categories.len() + spec.all_channels().count();
    if channel_count > MAX_CHANNELS {
        ctx.error(
            SpecPath::root(),
            format!("a server can have at most 500 channels including categories (spec lists {channel_count})"),
        );
    }
    check_unique(
        &mut ctx,
        "category",
        spec.categories.iter().enumerate().map(|(i, c)| (SpecPath::root().key("categories").index(i), c.name.as_str())),
    );
    for (i, cat) in spec.categories.iter().enumerate() {
        check_category(&mut ctx, i, cat, &role_names);
    }
    check_unique(
        &mut ctx,
        "channel",
        spec.channels.iter().enumerate().map(|(i, c)| (SpecPath::root().key("channels").index(i), c.name.as_str())),
    );
    for (i, ch) in spec.channels.iter().enumerate() {
        check_channel(&mut ctx, &SpecPath::root().key("channels").index(i), ch, false, &role_names);
    }
    ctx.out
}

/// Finds `prefix` `name` in a serde/toml message and suggests the closest of `candidates`.
fn did_you_mean(message: &str) -> Option<String> {
    let quoted = |after: &str| -> Option<String> {
        let start = message.find(after)? + after.len();
        let rest = &message[start..];
        let rest = rest.strip_prefix('`')?;
        Some(rest[..rest.find('`')?].to_owned())
    };
    if let Some(name) = quoted("unknown permission ") {
        let names = Permission::ALL.iter().map(|p| p.name());
        return suggest::closest(&name, names, 1).first().map(|s| format!("did you mean `{s}`?"));
    }
    let name = quoted("unknown field ").or_else(|| quoted("unknown variant "))?;
    let list = &message[message.find("expected one of ")? + "expected one of ".len()..];
    let expected: Vec<&str> = list.split(", ").map(|s| s.trim().trim_matches('`')).collect();
    suggest::closest(&name, expected, 1).first().map(|s| format!("did you mean `{s}`?"))
}

/// Parses and validates spec text, pointing every problem at its line and column.
#[must_use]
pub fn validate_text(text: &str) -> Report {
    match spec::parse(text) {
        Err(e) => Report {
            spec: None,
            problems: vec![Problem {
                severity: Severity::Error,
                path: String::new(),
                location: e.location,
                hint: did_you_mean(&e.message),
                message: e.message,
            }],
        },
        Ok(parsed) => {
            let source = SpecSource::new(text);
            let mut problems: Vec<Problem> = validate(&parsed)
                .into_iter()
                .map(|(severity, path, message, hint)| Problem {
                    severity,
                    location: source.locate(&path),
                    path: path.to_string(),
                    message,
                    hint,
                })
                .collect();
            problems.sort_by_key(|p| (p.location, p.severity));
            Report { spec: Some(parsed), problems }
        }
    }
}
