//! Export of the kinds beyond roles and channels (US9): live state → spec sections. Files are
//! exported as their CDN `url`; the app downloads them into `assets/<kind>/` when it can.
//! Webhook tokens and URLs are never exported (research E8), and only the bot's own invites
//! are (the planner manages no others).

use crate::asset::Asset;
use crate::id::Id;
use crate::live::{EventLocation, ExpressionKind, LiveExtras, avatar_url, event_cover_url, guild_image_url};
use crate::planner::channel_display;
use crate::snapshot::{LiveChannelKind, Snapshot};
use crate::spec::GuildSpec;
use crate::spec::automod::{Action, AutomodRule};
use crate::spec::events::{EventSpec, LocationSpec, SpecTime, from_rule};
use crate::spec::expressions::{EmojiSpec, SoundSpec, StickerSpec, Volume};
use crate::spec::invites::InviteSpec;
use crate::spec::onboarding::{OnboardingMode, OnboardingSpec, OptionSpec, PromptKind, PromptSpec};
use crate::spec::settings::{
    ContentFilter, FeaturesSpec, Notifications, SettingsSpec, VerificationLevel, system_flag_names,
};
use crate::spec::webhooks::WebhookSpec;
use crate::spec::welcome::{WelcomeChannelSpec, WelcomeScreenSpec};
use crate::time;

/// A channel as a spec reference: `#name` (or the plain name for voice), or its id when the
/// name is shared.
fn channel(s: &Snapshot, id: Id) -> String {
    let Some(c) = s.channel(id) else { return id.to_string() };
    let shared =
        s.channels.iter().filter(|x| x.name == c.name && matches!(x.kind, LiveChannelKind::Known(_))).count() > 1;
    if shared { id.to_string() } else { channel_display(c.kind, &c.name) }
}

/// A role as a spec reference: `@Name`, or its id when the name is shared.
fn role(s: &Snapshot, id: Id) -> String {
    if id == s.guild.id {
        return "@everyone".to_owned();
    }
    let Some(r) = s.role(id) else { return id.to_string() };
    if s.roles.iter().filter(|x| x.name == r.name).count() > 1 { id.to_string() } else { format!("@{}", r.name) }
}

fn url(u: String) -> Asset {
    Asset::Url { url: u }
}

fn settings(s: &Snapshot, x: &LiveExtras) -> Option<SettingsSpec> {
    let g = x.settings.as_ref()?;
    let gid = s.guild.id;
    let ch = |c: Option<Id>| Some(c.map_or_else(String::new, |c| channel(s, c)));
    Some(SettingsSpec {
        name: None,
        description: Some(g.description.clone().unwrap_or_default()),
        icon: g.icon.as_ref().map(|h| url(guild_image_url("icons", gid, h))),
        banner: g.banner.as_ref().map(|h| url(guild_image_url("banners", gid, h))),
        splash: g.splash.as_ref().map(|h| url(guild_image_url("splashes", gid, h))),
        system_channel: ch(g.system_channel_id),
        system_channel_flags: Some(system_flag_names(g.system_channel_flags)),
        rules_channel: ch(g.rules_channel_id),
        public_updates_channel: ch(g.public_updates_channel_id),
        safety_alerts_channel: ch(g.safety_alerts_channel_id),
        afk_channel: ch(g.afk_channel_id),
        afk_timeout: Some(g.afk_timeout),
        verification_level: VerificationLevel::from_code(g.verification_level),
        default_notifications: Notifications::from_code(g.default_notifications),
        explicit_content_filter: ContentFilter::from_code(g.explicit_content_filter),
        preferred_locale: Some(g.preferred_locale.clone()),
        premium_progress_bar: Some(g.premium_progress_bar),
        features: Some(FeaturesSpec {
            community: Some(g.features.contains("COMMUNITY")),
            invites_disabled: Some(g.features.contains("INVITES_DISABLED")),
            raid_alerts_disabled: Some(g.features.contains("RAID_ALERTS_DISABLED")),
            discoverable: Some(g.features.contains("DISCOVERABLE")),
        }),
    })
}

fn automod(s: &Snapshot, r: &AutomodRule<Id>) -> AutomodRule<String> {
    AutomodRule {
        id: r.id,
        name: r.name.clone(),
        enabled: r.enabled,
        trigger: r.trigger.clone(),
        actions: r
            .actions
            .iter()
            .map(|a| match a {
                Action::Block { message } => Action::Block { message: message.clone() },
                Action::Alert { channel: c } => Action::Alert { channel: channel(s, *c) },
                Action::Timeout { duration } => Action::Timeout { duration: *duration },
                Action::BlockInteraction => Action::BlockInteraction,
            })
            .collect(),
        exempt_roles: r.exempt_roles.iter().map(|id| role(s, *id)).collect(),
        exempt_channels: r.exempt_channels.iter().map(|id| channel(s, *id)).collect(),
    }
}

fn onboarding(s: &Snapshot, x: &LiveExtras) -> Option<OnboardingSpec> {
    let o = x.onboarding.as_ref()?;
    if !o.enabled && o.prompts.is_empty() && o.default_channel_ids.is_empty() {
        return None;
    }
    Some(OnboardingSpec {
        enabled: o.enabled,
        mode: if o.mode == 1 { OnboardingMode::Advanced } else { OnboardingMode::DefaultChannels },
        default_channels: o.default_channel_ids.iter().map(|c| channel(s, *c)).collect(),
        prompts: o
            .prompts
            .iter()
            .map(|p| PromptSpec {
                id: Some(p.id),
                title: p.title.clone(),
                kind: if p.kind == 1 { PromptKind::Dropdown } else { PromptKind::MultipleChoice },
                single_select: p.single_select,
                required: p.required,
                in_onboarding: p.in_onboarding,
                options: p
                    .options
                    .iter()
                    .map(|o| OptionSpec {
                        id: Some(o.id),
                        title: o.title.clone(),
                        description: o.description.clone(),
                        emoji: o.emoji.clone(),
                        roles: o.role_ids.iter().map(|r| role(s, *r)).collect(),
                        channels: o.channel_ids.iter().map(|c| channel(s, *c)).collect(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

fn welcome(s: &Snapshot, x: &LiveExtras) -> Option<WelcomeScreenSpec> {
    let w = x.welcome_screen.as_ref()?;
    if !w.enabled && w.channels.is_empty() && w.description.is_none() {
        return None;
    }
    Some(WelcomeScreenSpec {
        enabled: w.enabled,
        description: w.description.clone(),
        channels: w
            .channels
            .iter()
            .map(|c| WelcomeChannelSpec {
                channel: channel(s, c.channel_id),
                description: c.description.clone(),
                emoji: c.emoji.clone(),
            })
            .collect(),
    })
}

fn expressions(s: &Snapshot, x: &LiveExtras, spec: &mut GuildSpec) {
    for e in x.expressions.iter().flatten() {
        match e.kind {
            ExpressionKind::Emoji => spec.emoji.push(EmojiSpec {
                id: Some(e.id),
                name: e.name.clone(),
                image: url(e.url.clone()),
                roles: e.roles.iter().map(|r| role(s, *r)).collect(),
            }),
            ExpressionKind::Sticker => spec.stickers.push(StickerSpec {
                id: Some(e.id),
                name: e.name.clone(),
                description: e.description.clone().unwrap_or_default(),
                tags: e.tags.clone().unwrap_or_default(),
                image: url(e.url.clone()),
            }),
            ExpressionKind::Sound => spec.sounds.push(SoundSpec {
                id: Some(e.id),
                name: e.name.clone(),
                sound: url(e.url.clone()),
                volume: e.volume.map(Volume),
                emoji: e.emoji.clone(),
            }),
        }
    }
}

fn events(s: &Snapshot, x: &LiveExtras, spec: &mut GuildSpec) {
    for e in x.events.iter().flatten().filter(|e| matches!(e.status.as_str(), "scheduled" | "active")) {
        let Some(start) = time::parse_rfc3339(&e.start) else { continue };
        spec.events.push(EventSpec {
            id: Some(e.id),
            name: e.name.clone(),
            description: e.description.clone(),
            start: SpecTime::utc(start),
            end: e.end.as_deref().and_then(time::parse_rfc3339).map(SpecTime::utc),
            location: match &e.location {
                EventLocation::Voice(c) => LocationSpec::Voice(channel(s, *c)),
                EventLocation::Stage(c) => LocationSpec::Stage(channel(s, *c)),
                EventLocation::External(p) => LocationSpec::External { place: p.clone() },
            },
            recurrence: e.recurrence.as_ref().and_then(from_rule),
            image: e.image.as_ref().map(|h| url(event_cover_url(e.id, h))),
        });
    }
}

/// Adds every kind read in `x` to `spec` (which already holds the server's structure).
pub fn add_kinds(s: &Snapshot, x: &LiveExtras, spec: &mut GuildSpec) {
    if let Some(settings) = settings(s, x) {
        spec.guild.manage_settings = true;
        spec.guild.settings = Some(settings);
    }
    spec.automod = x.automod.iter().flatten().map(|r| automod(s, r)).collect();
    spec.onboarding = onboarding(s, x);
    spec.welcome_screen = welcome(s, x);
    expressions(s, x, spec);
    spec.webhooks = x
        .webhooks
        .iter()
        .flatten()
        .filter(|w| w.kind == crate::live::WebhookKind::Incoming)
        .map(|w| WebhookSpec {
            id: Some(w.id),
            name: w.name.clone(),
            channel: channel(s, w.channel_id),
            avatar: w.avatar.as_ref().map(|h| url(avatar_url(w.id, h))),
        })
        .collect();
    spec.invites = x
        .invites
        .iter()
        .flatten()
        .filter(|i| i.inviter_id == Some(s.bot.user_id))
        .map(|i| InviteSpec {
            code: Some(i.code.clone()),
            channel: channel(s, i.channel_id),
            max_age: i.max_age,
            max_uses: i.max_uses,
            temporary: i.temporary,
            roles: i.roles.iter().map(|r| role(s, *r)).collect(),
            recreate: false,
        })
        .collect();
    events(s, x, spec);
}
