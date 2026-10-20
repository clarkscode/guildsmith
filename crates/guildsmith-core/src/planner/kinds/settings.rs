//! `[guild.settings]` → one `UpdateSettings` op. Turning Community off or Discovery on is
//! destructive (data-model §5).

use std::collections::BTreeSet;

use super::{Refs, SpecAssets, change, changed, spec_hash, summary};
use crate::asset::Asset;
use crate::id::Id;
use crate::live::{LiveExtras, asset_key};
use crate::planner::Builder;
use crate::planner::ops::{FileOp, OpKind, Ref, Risk, SettingsSet};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::settings::{SettingsSpec, system_flags};

fn path(field: &str) -> SpecPath {
    SpecPath::root().key("guild").key("settings").key(field)
}

/// `Some(new)` when the image changes: `Some(None)` removes it.
#[allow(clippy::option_option)] // three states: leave alone, remove, set
fn image(
    b: &mut Builder<'_>,
    assets: &SpecAssets,
    x: &LiveExtras,
    field: &str,
    want: Option<&Asset>,
    live: Option<&String>,
) -> Option<Option<FileOp>> {
    let want = want?;
    if let Asset::Data { data } = want
        && data.is_empty()
    {
        return live.is_some().then_some(None);
    }
    let hash = spec_hash(b, assets, &path(field))?.clone();
    let same = live.is_some() && x.asset_hashes.get(&asset_key("settings", field)) == Some(&hash);
    (!same).then(|| Some(FileOp { asset: want.clone(), hash }))
}

/// A channel field: `""` clears it, anything else is a reference.
#[allow(clippy::option_option)] // three states: leave alone, clear, set
fn channel(
    b: &mut Builder<'_>,
    refs: &Refs,
    field: &str,
    want: Option<&String>,
    live: Option<Id>,
) -> Option<Option<Ref>> {
    let want = want?;
    if want.trim().is_empty() {
        return live.is_some().then_some(None);
    }
    match b.channel_ref(refs, want) {
        Ok(r) => (Some(&r) != live.map(Ref::Id).as_ref()).then_some(Some(r)),
        Err(e) => {
            b.errors.push(format!("{}: {e}", path(field)));
            None
        }
    }
}

fn shown(r: Option<&Option<Ref>>) -> String {
    match r {
        Some(Some(r)) => r.to_string(),
        _ => "none".to_owned(),
    }
}

fn channels(b: &mut Builder<'_>, refs: &Refs, s: &SettingsSpec, x: &crate::live::GuildSettings, set: &mut SettingsSet) {
    set.system_channel = channel(b, refs, "system_channel", s.system_channel.as_ref(), x.system_channel_id);
    set.rules_channel = channel(b, refs, "rules_channel", s.rules_channel.as_ref(), x.rules_channel_id);
    set.public_updates_channel =
        channel(b, refs, "public_updates_channel", s.public_updates_channel.as_ref(), x.public_updates_channel_id);
    set.safety_alerts_channel =
        channel(b, refs, "safety_alerts_channel", s.safety_alerts_channel.as_ref(), x.safety_alerts_channel_id);
    set.afk_channel = channel(b, refs, "afk_channel", s.afk_channel.as_ref(), x.afk_channel_id);
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras, assets: &SpecAssets) {
    let (Some(s), Some(live)) = (b.spec.guild.settings.as_ref().filter(|_| b.spec.guild.manage_settings), &x.settings)
    else {
        return;
    };
    let mut set = SettingsSet::default();
    let mut ch = Vec::new();
    if let Some(n) = &s.name
        && change(&mut ch, "name", &live.name, n.trim())
    {
        set.name = Some(n.trim().to_owned());
    }
    if let Some(d) = &s.description {
        let want = (!d.is_empty()).then(|| d.clone());
        if want != live.description {
            changed(&mut ch, "description");
            set.description = Some(want);
        }
    }
    set.icon = image(b, assets, x, "icon", s.icon.as_ref(), live.icon.as_ref());
    set.banner = image(b, assets, x, "banner", s.banner.as_ref(), live.banner.as_ref());
    set.splash = image(b, assets, x, "splash", s.splash.as_ref(), live.splash.as_ref());
    for (f, v) in [("icon", &set.icon), ("banner", &set.banner), ("splash", &set.splash)] {
        if v.is_some() {
            changed(&mut ch, f);
        }
    }
    channels(b, refs, s, live, &mut set);
    for (f, v, old) in [
        ("system_channel", &set.system_channel, live.system_channel_id),
        ("rules_channel", &set.rules_channel, live.rules_channel_id),
        ("public_updates_channel", &set.public_updates_channel, live.public_updates_channel_id),
        ("safety_alerts_channel", &set.safety_alerts_channel, live.safety_alerts_channel_id),
        ("afk_channel", &set.afk_channel, live.afk_channel_id),
    ] {
        if v.is_some() {
            change(&mut ch, f, old.map_or_else(|| "none".to_owned(), |i| i.to_string()), shown(v.as_ref()));
        }
    }
    if let Some(f) = &s.system_channel_flags
        && let Ok(bits) = system_flags(f)
        && bits != live.system_channel_flags
    {
        change(&mut ch, "system_channel_flags", live.system_channel_flags, bits);
        set.system_channel_flags = Some(bits);
    }
    scalars(s, live, &mut set, &mut ch);
    let risk = features(s, live, &mut set, &mut ch);
    community_rules(b, live, &set);
    if ch.is_empty() {
        return;
    }
    let target = Ref::Id(b.s.guild.id);
    b.op(
        Phase::Settings,
        summary("update server settings".to_owned(), &ch),
        risk,
        target,
        OpKind::UpdateSettings { changes: ch, set: Box::new(set) },
    );
}

fn scalars(
    s: &SettingsSpec,
    live: &crate::live::GuildSettings,
    set: &mut SettingsSet,
    ch: &mut Vec<super::FieldChange>,
) {
    if let Some(t) = s.afk_timeout
        && change(ch, "afk_timeout", live.afk_timeout, t)
    {
        set.afk_timeout = Some(t);
    }
    if let Some(v) = s.verification_level
        && change(ch, "verification_level", live.verification_level, v.code())
    {
        set.verification_level = Some(v.code());
    }
    if let Some(v) = s.default_notifications
        && change(ch, "default_notifications", live.default_notifications, v.code())
    {
        set.default_notifications = Some(v.code());
    }
    if let Some(v) = s.explicit_content_filter
        && change(ch, "explicit_content_filter", live.explicit_content_filter, v.code())
    {
        set.explicit_content_filter = Some(v.code());
    }
    if let Some(l) = &s.preferred_locale
        && change(ch, "preferred_locale", &live.preferred_locale, l)
    {
        set.preferred_locale = Some(l.clone());
    }
    if let Some(p) = s.premium_progress_bar
        && change(ch, "premium_progress_bar", live.premium_progress_bar, p)
    {
        set.premium_progress_bar = Some(p);
    }
}

/// The settable features; returns the risk (turning Community off or Discovery on loses
/// data or exposes the server).
fn features(
    s: &SettingsSpec,
    live: &crate::live::GuildSettings,
    set: &mut SettingsSet,
    ch: &mut Vec<super::FieldChange>,
) -> Risk {
    let Some(f) = &s.features else { return Risk::Safe };
    let mut new: BTreeSet<String> = live.features.clone();
    let mut risk = Risk::Safe;
    for (name, on) in f.wanted() {
        let had = live.features.contains(name);
        if had == on {
            continue;
        }
        change(ch, &format!("features.{}", name.to_ascii_lowercase()), had, on);
        if (name == "COMMUNITY" && !on) || (name == "DISCOVERABLE" && on) {
            risk = Risk::Destructive;
        }
        if on {
            new.insert(name.to_owned());
        } else {
            new.remove(name);
        }
    }
    if new != live.features {
        set.features = Some(new);
    }
    risk
}

/// Turning Community on needs a rules channel, a public updates channel, verification of at
/// least "low" and the content filter on for all members (Discord refuses otherwise).
fn community_rules(b: &mut Builder<'_>, live: &crate::live::GuildSettings, set: &SettingsSet) {
    let turning_on =
        set.features.as_ref().is_some_and(|f| f.contains("COMMUNITY")) && !live.features.contains("COMMUNITY");
    if !turning_on {
        return;
    }
    let has = |want: &Option<Option<Ref>>, old: Option<Id>| match want {
        Some(v) => v.is_some(),
        None => old.is_some(),
    };
    let mut missing = Vec::new();
    if !has(&set.rules_channel, live.rules_channel_id) {
        missing.push("rules_channel");
    }
    if !has(&set.public_updates_channel, live.public_updates_channel_id) {
        missing.push("public_updates_channel");
    }
    if set.verification_level.unwrap_or(live.verification_level) < 1 {
        missing.push("verification_level = \"low\" or higher");
    }
    if set.explicit_content_filter.unwrap_or(live.explicit_content_filter) < 2 {
        missing.push("explicit_content_filter = \"all_members\"");
    }
    if !missing.is_empty() {
        b.errors.push(format!("guild.settings: turning Community on needs {} (Discord's rule)", missing.join(", ")));
    }
}

/// The server's features once the planned settings run (onboarding needs Community).
pub(super) fn features_after(b: &Builder<'_>, x: &LiveExtras) -> BTreeSet<String> {
    let mut f = x.settings.as_ref().map_or_else(|| b.s.guild.features.clone(), |s| s.features.clone());
    if let Some(wanted) =
        b.spec.guild.settings.as_ref().filter(|_| b.spec.guild.manage_settings).and_then(|s| s.features.as_ref())
    {
        for (name, on) in wanted.wanted() {
            if on {
                f.insert(name.to_owned());
            } else {
                f.remove(name);
            }
        }
    }
    f
}
