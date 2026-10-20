//! `[onboarding]` and `[welcome_screen]`: each replaced in one request when it differs. Both
//! need a Community server; an enabled onboarding needs 7 default channels, 5 of which
//! `@everyone` can send in once the plan's structure changes are made.

use super::Refs;
use crate::explain::{self, Who};
use crate::id::Id;
use crate::live::{LiveExtras, Onboarding, WelcomeScreen};
use crate::permissions::Permission;
use crate::planner::Builder;
use crate::planner::ops::{OnboardingSet, OpKind, OptionSet, PromptSet, Ref, Risk, WelcomeChannelSet, WelcomeSet};
use crate::planner::order::{self, Phase};
use crate::planner::simulate;
use crate::spec::SpecPath;
use crate::spec::onboarding::{MIN_SENDABLE, OnboardingMode, OnboardingSpec, PromptKind};

fn resolve(b: &mut Builder<'_>, refs: &Refs, o: &OnboardingSpec, live: &Onboarding) -> Option<OnboardingSet> {
    let path = SpecPath::root().key("onboarding");
    let defaults =
        b.refs_or_err(&path.clone().key("default_channels"), &o.default_channels, |b, c| b.channel_ref(refs, c));
    let mut prompts = Vec::new();
    let mut ok = true;
    for (i, p) in o.prompts.iter().enumerate() {
        let pp = path.clone().key("prompts").index(i);
        let old =
            p.id.and_then(|id| live.prompts.iter().find(|x| x.id == id))
                .or_else(|| live.prompts.iter().find(|x| x.title == p.title.trim()));
        let mut options = Vec::new();
        for (j, opt) in p.options.iter().enumerate() {
            let op = pp.clone().key("options").index(j);
            let roles = b.refs_or_err(&op.clone().key("roles"), &opt.roles, Builder::role_ref);
            let channels = b.refs_or_err(&op.clone().key("channels"), &opt.channels, |b, c| b.channel_ref(refs, c));
            let (Some(roles), Some(channels)) = (roles, channels) else {
                ok = false;
                continue;
            };
            let old_o = old.and_then(|op| {
                opt.id
                    .and_then(|id| op.options.iter().find(|x| x.id == id))
                    .or_else(|| op.options.iter().find(|x| x.title == opt.title.trim()))
            });
            options.push(OptionSet {
                id: old_o.map(|x| x.id),
                title: opt.title.trim().to_owned(),
                description: opt.description.clone().filter(|d| !d.is_empty()),
                emoji: opt.emoji.clone(),
                roles,
                channels,
            });
        }
        prompts.push(PromptSet {
            id: old.map(|x| x.id),
            title: p.title.trim().to_owned(),
            kind: u8::from(p.kind == PromptKind::Dropdown),
            single_select: p.single_select,
            required: p.required,
            in_onboarding: p.in_onboarding,
            options,
        });
    }
    let defaults = defaults?;
    ok.then_some(OnboardingSet {
        enabled: o.enabled,
        mode: u8::from(o.mode == OnboardingMode::Advanced),
        default_channels: defaults,
        prompts,
    })
}

/// Whether `want` describes `live` (ids aside: new prompts and options get ids on apply).
fn same_onboarding(want: &OnboardingSet, live: &Onboarding) -> bool {
    let ids =
        |v: &[Ref]| v.iter().map(|r| if let Ref::Id(i) = r { Some(*i) } else { None }).collect::<Option<Vec<Id>>>();
    want.enabled == live.enabled
        && want.mode == live.mode
        && ids(&want.default_channels).is_some_and(|d| d == live.default_channel_ids)
        && want.prompts.len() == live.prompts.len()
        && want.prompts.iter().zip(&live.prompts).all(|(p, l)| {
            p.id == Some(l.id)
                && p.title == l.title
                && p.kind == l.kind
                && p.single_select == l.single_select
                && p.required == l.required
                && p.in_onboarding == l.in_onboarding
                && p.options.len() == l.options.len()
                && p.options.iter().zip(&l.options).all(|(o, lo)| {
                    o.id == Some(lo.id)
                        && o.title == lo.title
                        && o.description == lo.description
                        && o.emoji == lo.emoji
                        && ids(&o.roles).is_some_and(|r| r == lo.role_ids)
                        && ids(&o.channels).is_some_and(|c| c == lo.channel_ids)
                })
        })
}

/// Default channels `@everyone` can view and send in, after the plan's structure changes.
fn sendable(b: &Builder<'_>, defaults: &[Ref]) -> usize {
    let structure: Vec<_> = b.ops.iter().filter(|(p, _)| *p <= Phase::ChannelOrder).cloned().collect();
    let Ok((after, created)) = simulate::simulate_keyed(b.s, &order::order(structure)) else { return 0 };
    defaults
        .iter()
        .filter_map(|r| match r {
            Ref::Id(id) => Some(*id),
            Ref::New(k) => created.get(k).copied(),
        })
        .filter(|c| {
            explain::explain(&after, *c, &Who::Role(after.guild.id), None).is_ok_and(|e| {
                e.allowed.contains(Permission::ViewChannel) && e.allowed.contains(Permission::SendMessages)
            })
        })
        .count()
}

fn needs_community(b: &mut Builder<'_>, x: &LiveExtras, what: &str) -> bool {
    if super::settings::features_after(b, x).contains("COMMUNITY") {
        return true;
    }
    b.errors.push(format!(
        "{what} needs a Community server: set `features = {{ community = true }}` in [guild.settings] \
         (with manage_settings = true), or turn Community on in Discord"
    ));
    false
}

fn welcome(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras) {
    let (Some(w), Some(live)) = (b.spec.welcome_screen.clone(), &x.welcome_screen) else { return };
    let path = SpecPath::root().key("welcome_screen").key("channels");
    let mut channels = Vec::new();
    for (i, c) in w.channels.iter().enumerate() {
        match b.channel_ref(refs, &c.channel) {
            Ok(r) => channels.push(WelcomeChannelSet {
                channel: r,
                description: c.description.clone(),
                emoji: c.emoji.clone(),
            }),
            Err(e) => b.errors.push(format!("{}: {e}", path.clone().index(i))),
        }
    }
    let want =
        WelcomeSet { enabled: w.enabled, description: w.description.clone().filter(|d| !d.is_empty()), channels };
    if same_welcome(&want, live) || !needs_community(b, x, "the welcome screen") {
        return;
    }
    b.op(
        Phase::Onboarding,
        format!("set the welcome screen ({} channel(s))", want.channels.len()),
        Risk::Safe,
        Ref::Id(b.s.guild.id),
        OpKind::SetWelcomeScreen { screen: want },
    );
}

fn same_welcome(want: &WelcomeSet, live: &WelcomeScreen) -> bool {
    want.enabled == live.enabled
        && want.description == live.description
        && want.channels.len() == live.channels.len()
        && want
            .channels
            .iter()
            .zip(&live.channels)
            .all(|(w, l)| w.channel == Ref::Id(l.channel_id) && w.description == l.description && w.emoji == l.emoji)
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras) {
    welcome(b, refs, x);
    let (Some(o), Some(live)) = (b.spec.onboarding.clone(), &x.onboarding) else { return };
    let Some(want) = resolve(b, refs, &o, live) else { return };
    if same_onboarding(&want, live) || !needs_community(b, x, "onboarding") {
        return;
    }
    if want.enabled {
        let n = sendable(b, &want.default_channels);
        if n < MIN_SENDABLE {
            b.errors.push(format!(
                "onboarding: an enabled onboarding needs at least {MIN_SENDABLE} default channels @everyone can send \
                 messages in; {n} of these allow it"
            ));
            return;
        }
    }
    b.op(
        Phase::Onboarding,
        format!(
            "set onboarding: {}, {} default channel(s), {} prompt(s)",
            if want.enabled { "enabled" } else { "disabled" },
            want.default_channels.len(),
            want.prompts.len()
        ),
        Risk::Safe,
        Ref::Id(b.s.guild.id),
        OpKind::SetOnboarding { onboarding: want },
    );
}
