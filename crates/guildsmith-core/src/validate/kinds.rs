//! Offline checks for the kinds beyond roles and channels (US9), and the secret scan (E8).
//! References (`#channel`, `@Role`) are resolved by the planner, which also sees the server.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Ctx, check_unique};
use crate::asset::{self, Asset, AssetKind};
use crate::raw;
use crate::spec::automod::{self, AutomodRule};
use crate::spec::{GuildSpec, SpecPath};

fn root(k: &str) -> SpecPath {
    SpecPath::root().key(k)
}

fn each(ctx: &mut Ctx, path: &SpecPath, problems: Vec<String>) {
    for p in problems {
        ctx.error(path.clone(), p);
    }
}

/// Inline data can be checked without the server; files and URLs are read at plan time.
fn check_asset(ctx: &mut Ctx, path: SpecPath, a: &Asset, kind: AssetKind) {
    if let Asset::Data { data } = a
        && let Err(e) = asset::decode_data(data)
            .map_err(|e| e.to_string())
            .and_then(|bytes| asset::check(kind, &bytes).map(|_| ()).map_err(|e| e.to_string()))
    {
        ctx.error(path, e);
    }
}

fn check_settings(ctx: &mut Ctx, spec: &GuildSpec) {
    let Some(s) = &spec.guild.settings else { return };
    let path = root("guild").key("settings");
    if !spec.guild.manage_settings {
        ctx.error_hint(
            path.clone(),
            "[guild.settings] is used only when manage_settings = true",
            Some("add `manage_settings = true` under [guild], or remove [guild.settings]".to_owned()),
        );
    }
    each(ctx, &path.clone(), s.problems());
    for (field, a, kind) in [
        ("icon", &s.icon, AssetKind::GuildImage),
        ("banner", &s.banner, AssetKind::GuildImage),
        ("splash", &s.splash, AssetKind::GuildImage),
    ] {
        if let Some(a) = a {
            check_asset(ctx, path.clone().key(field), a, kind);
        }
    }
}

fn check_automod(ctx: &mut Ctx, rules: &[AutomodRule<String>]) {
    for (i, r) in rules.iter().enumerate() {
        each(ctx, &root("automod").index(i), automod::problems(r));
    }
    each(ctx, &root("automod"), automod::count_problems(rules.iter().map(|r| &r.trigger)));
    check_unique(
        ctx,
        "automod rule",
        rules.iter().enumerate().map(|(i, r)| (root("automod").index(i), r.name.as_str())),
    );
}

fn check_expressions(ctx: &mut Ctx, spec: &GuildSpec) {
    for (i, e) in spec.emoji.iter().enumerate() {
        each(ctx, &root("emoji").index(i), e.problems());
        check_asset(ctx, root("emoji").index(i).key("image"), &e.image, AssetKind::Emoji);
    }
    check_unique(ctx, "emoji", spec.emoji.iter().enumerate().map(|(i, e)| (root("emoji").index(i), e.name.as_str())));
    for (i, s) in spec.stickers.iter().enumerate() {
        each(ctx, &root("stickers").index(i), s.problems());
        check_asset(ctx, root("stickers").index(i).key("image"), &s.image, AssetKind::Sticker);
    }
    check_unique(
        ctx,
        "sticker",
        spec.stickers.iter().enumerate().map(|(i, s)| (root("stickers").index(i), s.name.as_str())),
    );
    for (i, s) in spec.sounds.iter().enumerate() {
        each(ctx, &root("sounds").index(i), s.problems());
        check_asset(ctx, root("sounds").index(i).key("sound"), &s.sound, AssetKind::Sound);
    }
    check_unique(ctx, "sound", spec.sounds.iter().enumerate().map(|(i, s)| (root("sounds").index(i), s.name.as_str())));
}

fn check_rest(ctx: &mut Ctx, spec: &GuildSpec) {
    if let Some(o) = &spec.onboarding {
        each(ctx, &root("onboarding"), o.problems());
    }
    if let Some(w) = &spec.welcome_screen {
        each(ctx, &root("welcome_screen"), w.problems());
    }
    for (i, w) in spec.webhooks.iter().enumerate() {
        each(ctx, &root("webhooks").index(i), w.problems());
        if let Some(a) = &w.avatar {
            check_asset(ctx, root("webhooks").index(i).key("avatar"), a, AssetKind::Avatar);
        }
    }
    check_unique(
        ctx,
        "webhook",
        spec.webhooks.iter().enumerate().map(|(i, w)| (root("webhooks").index(i), w.name.as_str())),
    );
    let mut signatures = BTreeSet::new();
    for (i, inv) in spec.invites.iter().enumerate() {
        each(ctx, &root("invites").index(i), inv.problems());
        let mut roles = inv.roles.clone();
        roles.sort();
        if inv.code.is_none() && !signatures.insert((&inv.channel, inv.max_age, inv.max_uses, inv.temporary, roles)) {
            ctx.error(
                root("invites").index(i),
                "two invites without a code have the same channel and settings, so they can't be told apart",
            );
        }
    }
    for (i, e) in spec.events.iter().enumerate() {
        each(ctx, &root("events").index(i), e.problems());
        if let Some(a) = &e.image {
            check_asset(ctx, root("events").index(i).key("image"), a, AssetKind::GuildImage);
        }
    }
    check_unique(ctx, "event", spec.events.iter().enumerate().map(|(i, e)| (root("events").index(i), e.name.as_str())));
    for (i, m) in spec.messages.iter().enumerate() {
        each(ctx, &root("messages").index(i), m.problems());
    }
}

/// A webhook token: 60–80 URL-safe base64 characters, mixing cases and digits.
fn looks_like_webhook_token(s: &str) -> bool {
    (60..=80).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && s.bytes().any(|b| b.is_ascii_uppercase())
        && s.bytes().any(|b| b.is_ascii_lowercase())
        && s.bytes().any(|b| b.is_ascii_digit())
}

fn scan(ctx: &mut Ctx, path: &SpecPath, v: &Value) {
    match v {
        Value::String(s) => {
            let what = if raw::is_webhook_url(s) {
                Some("a webhook URL")
            } else if raw::looks_like_bot_token(s) || looks_like_webhook_token(s) {
                Some("a token")
            } else {
                None
            };
            if let Some(what) = what {
                ctx.error_hint(
                    path.clone(),
                    format!("this looks like {what}; secrets never go in the spec file"),
                    Some("remove it: guildsmith shows a new webhook's URL once, in the apply result".to_owned()),
                );
            }
        }
        Value::Array(items) => {
            for (i, x) in items.iter().enumerate() {
                scan(ctx, &path.clone().index(i), x);
            }
        }
        Value::Object(map) => {
            for (k, x) in map {
                // Inline asset data is base64, not a secret.
                if k != "data" {
                    scan(ctx, &path.clone().key(k), x);
                }
            }
        }
        _ => {}
    }
}

/// Checks every kind beyond roles and channels, and scans the whole file for secrets (E8).
pub(super) fn check(ctx: &mut Ctx, spec: &GuildSpec) {
    check_settings(ctx, spec);
    check_automod(ctx, &spec.automod);
    check_expressions(ctx, spec);
    check_rest(ctx, spec);
    if let Ok(v) = serde_json::to_value(spec) {
        scan(ctx, &SpecPath::root(), &v);
    }
}
