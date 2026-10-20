//! The `settings` group (US7, FR-032–035): server settings, onboarding, the welcome screen,
//! templates (including import into a spec file), and integrations.
//!
//! Settings previews show each field as old → new. Bots can't create servers, so a template
//! is only ever imported into a file.

use std::collections::BTreeSet;
use std::path::Path;

use guildsmith_core::asset::{Asset, AssetKind, data_uri};
use guildsmith_core::explain::{self, Who};
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::spec::onboarding::{
    MIN_SENDABLE, OnboardingMode, OnboardingSpec, OptionSpec, PromptKind, PromptSpec,
};
use guildsmith_core::spec::settings::{
    ContentFilter, Notifications, SettingsSpec, VerificationLevel, system_flag_names, system_flags,
};
use guildsmith_core::spec::welcome::{WelcomeChannelSpec, WelcomeScreenSpec};
use guildsmith_core::spec::writer::{TemplateHeader, write_template};
use guildsmith_core::{Id, Permission, template};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;
use crate::assets::{self, Caller};
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::{Refusal, RefusalKind, audit_reason};
use crate::ports::{
    DiscordPort, GuildSettings, IntegrationInfo, Onboarding, OnboardingOption, OnboardingPrompt, SettingsChange,
    TemplateInfo, Vanity, WelcomeChannel, WelcomeScreen, WidgetSettings,
};
use crate::preflight::{self, PreflightProblem};
use crate::resolve;
use crate::services::export;
use crate::services::live::{Deleted, invalid, refuse_if, resolved, role_name, server, writable};

/// The explanation for "create a server from this template" (FR-035, US7 scenario 4).
pub const NO_SERVER_CREATION: &str = "bots can't create servers (Discord removed that for bots)";

// ---- shared ------------------------------------------------------------------------------------

fn channel_label(s: &Snapshot, id: Option<Id>) -> String {
    id.map_or_else(|| "none".to_owned(), |c| s.channel(c).map_or_else(|| c.to_string(), |c| format!("#{}", c.name)))
}

/// `#name`, an id, or `""` (clear).
#[allow(clippy::option_option)] // three states: not given, clear, set
fn channel_ref(s: &Snapshot, input: Option<&str>) -> Result<Option<Option<Id>>, ServiceError> {
    match input.map(str::trim) {
        None => Ok(None),
        Some("" | "none") => Ok(Some(None)),
        Some(c) => Ok(Some(Some(resolved(resolve::resolve_channel(s, c), "channel", c)?))),
    }
}

fn need_admin(s: &Snapshot, why: &str) -> Option<PreflightProblem> {
    let ok = s.bot.permissions.contains(Permission::Administrator) || s.bot.user_id == s.guild.owner_id;
    (!ok).then(|| PreflightProblem::MissingPermission {
        perm: Permission::Administrator.name().to_owned(),
        needed_by: why.to_owned(),
    })
}

fn enum_name<T: Serialize>(v: T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

// ---- settings_get --------------------------------------------------------------------------------

/// Which part of the settings to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    General,
    Onboarding,
    WelcomeScreen,
    Widget,
    Integrations,
    Vanity,
}

/// General settings with names instead of ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)] // one flag per image, plus the progress bar
pub struct GeneralView {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub icon: bool,
    pub banner: bool,
    pub splash: bool,
    pub system_channel: String,
    pub system_channel_flags: Vec<String>,
    pub rules_channel: String,
    pub public_updates_channel: String,
    pub safety_alerts_channel: String,
    pub afk_channel: String,
    pub afk_timeout: u32,
    pub verification_level: String,
    pub default_notifications: String,
    pub explicit_content_filter: String,
    pub preferred_locale: String,
    pub premium_progress_bar: bool,
    pub features: Vec<String>,
}

fn general(s: &Snapshot, g: &GuildSettings) -> GeneralView {
    GeneralView {
        name: g.name.clone(),
        description: g.description.clone(),
        icon: g.icon.is_some(),
        banner: g.banner.is_some(),
        splash: g.splash.is_some(),
        system_channel: channel_label(s, g.system_channel_id),
        system_channel_flags: system_flag_names(g.system_channel_flags),
        rules_channel: channel_label(s, g.rules_channel_id),
        public_updates_channel: channel_label(s, g.public_updates_channel_id),
        safety_alerts_channel: channel_label(s, g.safety_alerts_channel_id),
        afk_channel: channel_label(s, g.afk_channel_id),
        afk_timeout: g.afk_timeout,
        verification_level: VerificationLevel::from_code(g.verification_level)
            .map_or_else(|| g.verification_level.to_string(), enum_name),
        default_notifications: Notifications::from_code(g.default_notifications)
            .map_or_else(|| g.default_notifications.to_string(), enum_name),
        explicit_content_filter: ContentFilter::from_code(g.explicit_content_filter)
            .map_or_else(|| g.explicit_content_filter.to_string(), enum_name),
        preferred_locale: g.preferred_locale.clone(),
        premium_progress_bar: g.premium_progress_bar,
        features: g.features.iter().cloned().collect(),
    }
}

fn onboarding_view(s: &Snapshot, o: &Onboarding) -> OnboardingSpec {
    let ch = |id: &Id| channel_label(s, Some(*id));
    OnboardingSpec {
        enabled: o.enabled,
        mode: if o.mode == 1 { OnboardingMode::Advanced } else { OnboardingMode::DefaultChannels },
        default_channels: o.default_channel_ids.iter().map(ch).collect(),
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
                    .map(|x| OptionSpec {
                        id: Some(x.id),
                        title: x.title.clone(),
                        description: x.description.clone(),
                        emoji: x.emoji.clone(),
                        roles: x.role_ids.iter().map(|r| role_name(s, *r)).collect(),
                        channels: x.channel_ids.iter().map(ch).collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn welcome_view(s: &Snapshot, w: &WelcomeScreen) -> WelcomeScreenSpec {
    WelcomeScreenSpec {
        enabled: w.enabled,
        description: w.description.clone(),
        channels: w
            .channels
            .iter()
            .map(|c| WelcomeChannelSpec {
                channel: channel_label(s, Some(c.channel_id)),
                description: c.description.clone(),
                emoji: c.emoji.clone(),
            })
            .collect(),
    }
}

/// The server's settings, or one section of them.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SettingsView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub general: Option<GeneralView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding: Option<OnboardingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcome_screen: Option<WelcomeScreenSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<WidgetSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrations: Option<Vec<IntegrationInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vanity: Option<Vanity>,
}

/// Reads settings: everything, or one section.
pub async fn settings_get<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    section: Option<SettingsSection>,
) -> Result<SettingsView, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["settings_get"]))?;
    let want = |x: SettingsSection| section.is_none_or(|w| w == x);
    let mut v = SettingsView {
        general: None,
        onboarding: None,
        welcome_screen: None,
        widget: None,
        integrations: None,
        vanity: None,
    };
    if want(SettingsSection::General) {
        v.general = Some(general(&s, &app.port.guild_settings(gid).await?));
    }
    if want(SettingsSection::Onboarding) {
        v.onboarding = Some(onboarding_view(&s, &app.port.onboarding(gid).await?));
    }
    if want(SettingsSection::WelcomeScreen) {
        v.welcome_screen = Some(welcome_view(&s, &app.port.welcome_screen(gid).await?));
    }
    if want(SettingsSection::Widget) {
        v.widget = Some(app.port.widget(gid).await?);
    }
    if want(SettingsSection::Integrations) {
        v.integrations = Some(app.port.integrations(gid).await?);
    }
    if want(SettingsSection::Vanity) {
        v.vanity = Some(app.port.vanity(gid).await?);
    }
    Ok(v)
}

// ---- settings_update -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SettingsUpdated {
    /// `field old → new`; empty when nothing needed to change.
    pub changes: Vec<String>,
}

async fn image<P: DiscordPort>(app: &App<P>, a: Option<&Asset>) -> Result<Option<String>, ServiceError> {
    match a {
        Some(a) => {
            let l = assets::load(&app.port, &app.policy, Caller::Mcp { base: None }, a, AssetKind::GuildImage).await?;
            Ok(Some(data_uri(l.file_type, &l.bytes)))
        }
        None => Ok(None),
    }
}

/// The change and its `old → new` lines (shared with spec apply).
pub(crate) async fn settings_change<P: DiscordPort>(
    app: &App<P>,
    s: &Snapshot,
    cur: &GuildSettings,
    spec: &SettingsSpec,
) -> Result<(SettingsChange, Vec<String>, Vec<PreflightProblem>), ServiceError> {
    let problems_in = spec.problems();
    if !problems_in.is_empty() {
        return Err(invalid(problems_in.join("; ")));
    }
    let mut c = SettingsChange::default();
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    if let Some(n) = spec.name.as_ref().map(|n| n.trim().to_owned()).filter(|n| *n != cur.name) {
        lines.push(format!("name {:?} → {n:?}", cur.name));
        c.name = Some(n);
    }
    if let Some(d) = &spec.description {
        let want = (!d.is_empty()).then(|| d.clone());
        if want != cur.description {
            lines.push(format!("description {:?} → {:?}", cur.description.as_deref().unwrap_or(""), d));
            c.description = Some(want);
        }
    }
    for (label, asset, slot) in [
        ("icon", &spec.icon, &mut c.icon),
        ("banner", &spec.banner, &mut c.banner),
        ("splash", &spec.splash, &mut c.splash),
    ] {
        if let Some(uri) = image(app, asset.as_ref()).await? {
            lines.push(format!("{label} replaced"));
            *slot = Some(Some(uri));
        }
    }
    let mut channel = |label: &str,
                       input: &Option<String>,
                       current: Option<Id>,
                       slot: &mut Option<Option<Id>>|
     -> Result<(), ServiceError> {
        if let Some(want) = channel_ref(s, input.as_deref())?
            && want != current
        {
            lines.push(format!("{label} {} → {}", channel_label(s, current), channel_label(s, want)));
            *slot = Some(want);
        }
        Ok(())
    };
    channel("system_channel", &spec.system_channel, cur.system_channel_id, &mut c.system_channel_id)?;
    channel("rules_channel", &spec.rules_channel, cur.rules_channel_id, &mut c.rules_channel_id)?;
    channel(
        "public_updates_channel",
        &spec.public_updates_channel,
        cur.public_updates_channel_id,
        &mut c.public_updates_channel_id,
    )?;
    channel(
        "safety_alerts_channel",
        &spec.safety_alerts_channel,
        cur.safety_alerts_channel_id,
        &mut c.safety_alerts_channel_id,
    )?;
    channel("afk_channel", &spec.afk_channel, cur.afk_channel_id, &mut c.afk_channel_id)?;
    if let Some(Some(afk)) = c.afk_channel_id
        && !matches!(
            s.channel(afk).map(|x| x.kind),
            Some(LiveChannelKind::Known(guildsmith_core::spec::ChannelKind::Voice))
        )
    {
        return Err(invalid("the AFK channel must be a voice channel"));
    }
    if let Some(flags) = &spec.system_channel_flags {
        let bits = system_flags(flags).map_err(|n| invalid(format!("unknown system channel flag `{n}`")))?;
        if bits != cur.system_channel_flags {
            lines.push(format!(
                "system_channel_flags [{}] → [{}]",
                system_flag_names(cur.system_channel_flags).join(", "),
                flags.join(", ")
            ));
            c.system_channel_flags = Some(bits);
        }
    }
    level_changes(cur, spec, &mut c, &mut lines);
    if let Some(f) = &spec.features {
        features_change(s, cur, *f, &mut c, &mut lines, &mut problems)?;
    }
    Ok((c, lines, problems))
}

/// AFK timeout, verification, notifications, content filter, locale and the progress bar.
fn level_changes(cur: &GuildSettings, spec: &SettingsSpec, c: &mut SettingsChange, lines: &mut Vec<String>) {
    if let Some(t) = spec.afk_timeout.filter(|t| *t != cur.afk_timeout) {
        lines.push(format!("afk_timeout {} → {t}", cur.afk_timeout));
        c.afk_timeout = Some(t);
    }
    if let Some(v) = spec.verification_level.filter(|v| v.code() != cur.verification_level) {
        let old = VerificationLevel::from_code(cur.verification_level).map_or_else(String::new, enum_name);
        lines.push(format!("verification_level {old} → {}", enum_name(v)));
        c.verification_level = Some(v.code());
    }
    if let Some(v) = spec.default_notifications.filter(|v| v.code() != cur.default_notifications) {
        let old = Notifications::from_code(cur.default_notifications).map_or_else(String::new, enum_name);
        lines.push(format!("default_notifications {old} → {}", enum_name(v)));
        c.default_notifications = Some(v.code());
    }
    if let Some(v) = spec.explicit_content_filter.filter(|v| v.code() != cur.explicit_content_filter) {
        let old = ContentFilter::from_code(cur.explicit_content_filter).map_or_else(String::new, enum_name);
        lines.push(format!("explicit_content_filter {old} → {}", enum_name(v)));
        c.explicit_content_filter = Some(v.code());
    }
    if let Some(l) = spec.preferred_locale.as_ref().filter(|l| **l != cur.preferred_locale) {
        lines.push(format!("preferred_locale {} → {l}", cur.preferred_locale));
        c.preferred_locale = Some(l.clone());
    }
    if let Some(b) = spec.premium_progress_bar.filter(|b| *b != cur.premium_progress_bar) {
        lines.push(format!("premium_progress_bar {} → {b}", cur.premium_progress_bar));
        c.premium_progress_bar = Some(b);
    }
}

/// Feature switches: the new feature list, its `old → new` lines, and the Administrator need
/// for COMMUNITY and DISCOVERABLE. A community server needs rules and updates channels.
fn features_change(
    s: &Snapshot,
    cur: &GuildSettings,
    f: guildsmith_core::spec::settings::FeaturesSpec,
    c: &mut SettingsChange,
    lines: &mut Vec<String>,
    problems: &mut Vec<PreflightProblem>,
) -> Result<(), ServiceError> {
    let mut features: BTreeSet<String> = cur.features.clone();
    for (name, on) in f.wanted() {
        if features.contains(name) == on {
            continue;
        }
        lines.push(format!("{} {} → {on}", name.to_lowercase(), !on));
        if on {
            features.insert(name.to_owned());
        } else {
            features.remove(name);
        }
        if name == "COMMUNITY" || name == "DISCOVERABLE" {
            problems
                .extend(need_admin(s, &format!("turning {} {}", name.to_lowercase(), if on { "on" } else { "off" })));
        }
    }
    if features != cur.features {
        if features.contains("COMMUNITY") && !cur.features.contains("COMMUNITY") {
            let rules = c.rules_channel_id.unwrap_or(cur.rules_channel_id);
            let updates = c.public_updates_channel_id.unwrap_or(cur.public_updates_channel_id);
            if rules.is_none() || updates.is_none() {
                return Err(invalid("a community server needs rules_channel and public_updates_channel set"));
            }
        }
        c.features = Some(features);
    }
    Ok(())
}

/// Changes server settings after confirmation. The preview shows each field as old → new.
pub async fn settings_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    spec: &SettingsSpec,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<SettingsUpdated>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let mut problems = preflight::check_tools(&s, ["settings_update"]);
    let cur = app.port.guild_settings(gid).await?;
    let (change, lines, more) = settings_change(app, &s, &cur, spec).await?;
    problems.extend(more);
    refuse_if(&problems)?;
    if lines.is_empty() {
        return Ok(Outcome::Done(SettingsUpdated { changes: lines }));
    }
    confirm::guarded(confirmed, lines.clone(), || async {
        let summary = format!("update settings: {}", lines.join(", "));
        app.port.update_guild_settings(gid, &change, &audit_reason(&summary, reason)).await?;
        app.invalidate(gid);
        Ok(SettingsUpdated { changes: lines.clone() })
    })
    .await
}

// ---- onboarding and welcome screen ---------------------------------------------------------------

/// A snowflake for a new onboarding prompt or option (Discord wants client-made ids).
pub(crate) fn new_id(now_ms: i64, n: &mut u64) -> Id {
    *n += 1;
    Id((u64::try_from(now_ms - 1_420_070_400_000).unwrap_or(0) << 22) + *n)
}

/// Resolves an onboarding spec against the server, keeping the ids of existing prompts and
/// options (matched by id, then title). Checks the default-channel rule (shared with spec apply).
pub(crate) fn resolve_onboarding(
    s: &Snapshot,
    spec: &OnboardingSpec,
    current: &Onboarding,
    now_ms: i64,
) -> Result<Onboarding, ServiceError> {
    let problems = spec.problems();
    if !problems.is_empty() {
        return Err(invalid(problems.join("; ")));
    }
    if spec.enabled && !s.guild.features.contains("COMMUNITY") {
        let p =
            PreflightProblem::MissingFeature { feature: "COMMUNITY".to_owned(), needed_by: "onboarding".to_owned() };
        return Err(crate::services::apply::preflight_refusal(&[p]).into());
    }
    let ch = |c: &String| resolved(resolve::resolve_channel(s, c), "channel", c);
    let role = |r: &String| resolved(resolve::resolve_role(s, r), "role", r);
    let defaults = spec.default_channels.iter().map(ch).collect::<Result<Vec<Id>, _>>()?;
    if spec.enabled {
        let sendable = defaults
            .iter()
            .filter(|c| {
                explain::explain(s, **c, &Who::Role(s.guild.id), None).is_ok_and(|e| {
                    e.allowed.contains(Permission::ViewChannel) && e.allowed.contains(Permission::SendMessages)
                })
            })
            .count();
        if sendable < MIN_SENDABLE {
            return Err(invalid(format!(
                "an enabled onboarding needs at least {MIN_SENDABLE} default channels @everyone can send messages in; \
                 {sendable} of these allow it"
            )));
        }
    }
    let mut n = 0;
    let mut prompts = Vec::new();
    for p in &spec.prompts {
        let old = current
            .prompts
            .iter()
            .find(|x| Some(x.id) == p.id)
            .or_else(|| current.prompts.iter().find(|x| x.title == p.title));
        let mut options = Vec::new();
        for o in &p.options {
            let old_o = old.and_then(|op| {
                op.options
                    .iter()
                    .find(|x| Some(x.id) == o.id)
                    .or_else(|| op.options.iter().find(|x| x.title == o.title))
            });
            options.push(OnboardingOption {
                id: old_o.map_or_else(|| new_id(now_ms, &mut n), |x| x.id),
                title: o.title.trim().to_owned(),
                description: o.description.clone(),
                emoji: o.emoji.clone(),
                role_ids: o.roles.iter().map(role).collect::<Result<_, _>>()?,
                channel_ids: o.channels.iter().map(ch).collect::<Result<_, _>>()?,
            });
        }
        prompts.push(OnboardingPrompt {
            id: old.map_or_else(|| new_id(now_ms, &mut n), |x| x.id),
            title: p.title.trim().to_owned(),
            kind: u8::from(p.kind == PromptKind::Dropdown),
            single_select: p.single_select,
            required: p.required,
            in_onboarding: p.in_onboarding,
            options,
        });
    }
    Ok(Onboarding {
        enabled: spec.enabled,
        mode: u8::from(spec.mode == OnboardingMode::Advanced),
        default_channel_ids: defaults,
        prompts,
    })
}

/// Replaces the onboarding (prompts, options, default channels) after confirmation. Unknown
/// roles or channels fail before anything changes.
pub async fn onboarding_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    spec: &OnboardingSpec,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<OnboardingSpec>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["onboarding_update"]))?;
    let current = app.port.onboarding(gid).await?;
    let now_ms = date::now_secs(app.clock.as_ref()) * 1000;
    let wanted = resolve_onboarding(&s, spec, &current, now_ms)?;
    let would = vec![format!(
        "replace onboarding: {}, {} default channel(s), {} prompt(s) (now {} prompt(s))",
        if wanted.enabled { "enabled" } else { "disabled" },
        wanted.default_channel_ids.len(),
        wanted.prompts.len(),
        current.prompts.len()
    )];
    confirm::guarded(confirmed, would, || async {
        let set = app.port.set_onboarding(gid, &wanted, &audit_reason("replace onboarding", reason)).await?;
        Ok(onboarding_view(&s, &set))
    })
    .await
}

/// Resolves a welcome screen spec (shared with spec apply).
pub(crate) fn resolve_welcome(s: &Snapshot, spec: &WelcomeScreenSpec) -> Result<WelcomeScreen, ServiceError> {
    let problems = spec.problems();
    if !problems.is_empty() {
        return Err(invalid(problems.join("; ")));
    }
    Ok(WelcomeScreen {
        enabled: spec.enabled,
        description: spec.description.clone().filter(|d| !d.is_empty()),
        channels: spec
            .channels
            .iter()
            .map(|c| {
                Ok(WelcomeChannel {
                    channel_id: resolved(resolve::resolve_channel(s, &c.channel), "channel", &c.channel)?,
                    description: c.description.trim().to_owned(),
                    emoji: c.emoji.clone(),
                })
            })
            .collect::<Result<_, ServiceError>>()?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WelcomeUpdated {
    pub welcome_screen: WelcomeScreenSpec,
    pub changed: bool,
}

/// Sets the welcome screen (up to 5 suggested channels). Nothing is sent when it already matches.
pub async fn welcome_screen_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    spec: &WelcomeScreenSpec,
    reason: Option<&str>,
) -> Result<WelcomeUpdated, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["welcome_screen_update"]))?;
    let wanted = resolve_welcome(&s, spec)?;
    let current = app.port.welcome_screen(gid).await?;
    if wanted == current {
        return Ok(WelcomeUpdated { welcome_screen: welcome_view(&s, &current), changed: false });
    }
    let set = app.port.set_welcome_screen(gid, &wanted, &audit_reason("update welcome screen", reason)).await?;
    Ok(WelcomeUpdated { welcome_screen: welcome_view(&s, &set), changed: true })
}

// ---- templates ---------------------------------------------------------------------------------

/// What `templates` does. `use` (make a server from it) is always refused: bots can't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateAction {
    List,
    Create,
    Sync,
    Update,
    Delete,
    Use,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TemplateRow {
    pub code: String,
    pub url: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub uses: u64,
    /// The server changed since the last sync.
    pub out_of_date: bool,
    pub updated_at: String,
}

fn template_row(t: &TemplateInfo) -> TemplateRow {
    TemplateRow {
        code: t.code.clone(),
        url: format!("https://discord.new/{}", t.code),
        name: t.name.clone(),
        description: t.description.clone(),
        uses: t.usage_count,
        out_of_date: t.dirty,
        updated_at: t.updated_at.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Templates {
    pub templates: Vec<TemplateRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
}

/// Lists, creates (one per server), syncs, updates or deletes (after confirmation) the server's
/// template. `use` explains that bots can't create servers and points to `template_import`.
pub async fn templates<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    action: TemplateAction,
    name: Option<&str>,
    description: Option<&str>,
    confirmed: bool,
) -> Result<Outcome<Templates>, ServiceError> {
    if action == TemplateAction::Use {
        return Err(Refusal::new(
            RefusalKind::Unsupported,
            format!("{NO_SERVER_CREATION}; nothing was done"),
            Some(
                "use template_import to turn the template into a guild.toml, then plan it against a server you created",
            ),
        )
        .into());
    }
    let (gid, s) =
        if action == TemplateAction::List { server(app, guild).await? } else { writable(app, guild).await? };
    refuse_if(&preflight::check_tools(&s, ["templates"]))?;
    let current = app.port.templates(gid).await?;
    let list = |done: Option<String>, all: &[TemplateInfo]| Templates {
        templates: all.iter().map(template_row).collect(),
        done,
    };
    let only = || current.first().cloned().ok_or_else(|| invalid("this server has no template; create one first"));
    match action {
        TemplateAction::List => Ok(Outcome::Done(list(None, &current))),
        TemplateAction::Create => {
            if !current.is_empty() {
                return Err(invalid(
                    "a server has one template, and this one already has it; sync or update it instead",
                ));
            }
            let n = name
                .map(str::trim)
                .filter(|n| (1..=100).contains(&n.chars().count()))
                .ok_or_else(|| invalid("`name` (1–100 characters) is needed"))?;
            if description.is_some_and(|d| d.chars().count() > 120) {
                return Err(invalid("a template description is at most 120 characters"));
            }
            let t = app.port.create_template(gid, n, description).await?;
            Ok(Outcome::Done(list(Some(format!("created template {}", t.code)), &[t])))
        }
        TemplateAction::Sync => {
            let t = app.port.sync_template(gid, &only()?.code).await?;
            Ok(Outcome::Done(list(Some(format!("synced template {} with the server", t.code)), &[t])))
        }
        TemplateAction::Update => {
            if name.is_none() && description.is_none() {
                return Err(invalid("give a new `name` and/or `description`"));
            }
            let t = app.port.update_template(gid, &only()?.code, name, description).await?;
            Ok(Outcome::Done(list(Some(format!("updated template {}", t.code)), &[t])))
        }
        TemplateAction::Delete => {
            let t = only()?;
            confirm::guarded(
                confirmed,
                vec![format!("delete template {} ({}); its link stops working", t.name, t.code)],
                || async {
                    app.port.delete_template(gid, &t.code).await?;
                    Ok(list(Some(format!("deleted template {}", t.code)), &[]))
                },
            )
            .await
        }
        TemplateAction::Use => unreachable!("handled above"),
    }
}

/// A template turned into a spec file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TemplateImport {
    pub toml: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub template: String,
    /// Channel kinds guildsmith doesn't manage, left as comments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmanaged: Vec<String>,
}

/// A template code from a code or a link (`discord.new/<code>`, `discord.com/template/<code>`).
#[must_use]
pub fn template_code(input: &str) -> String {
    input.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_owned()
}

/// Turns a template (code or link) into a spec file, optionally written to `path` (never
/// overwriting). Never creates a server.
pub async fn template_import<P: DiscordPort>(
    app: &App<P>,
    code_or_url: &str,
    path: Option<&Path>,
) -> Result<TemplateImport, ServiceError> {
    let code = template_code(code_or_url);
    if code.is_empty() {
        return Err(invalid("give a template code or link"));
    }
    let t = app.port.get_template(&code).await?;
    let (spec, notes) = template::import(&t.serialized_source_guild).map_err(|e| invalid(e.to_string()))?;
    let header = TemplateHeader {
        version: env!("CARGO_PKG_VERSION"),
        code: &code,
        template_name: &t.name,
        date: &date::today(app.clock.as_ref()),
    };
    let toml = write_template(&spec, &header, &notes);
    if let Some(p) = path {
        export::write_file(p, &toml, false)?;
    }
    Ok(TemplateImport {
        toml,
        path: path.map(|p| p.display().to_string()),
        template: t.name,
        unmanaged: notes.unmanaged,
    })
}

// ---- integrations --------------------------------------------------------------------------------

/// Removes an integration (by id or name) after confirmation; a bot integration's bot leaves.
pub async fn integration_remove<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    integration: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["integration_remove"]))?;
    let all = app.port.integrations(gid).await?;
    let raw = integration.trim();
    let hits: Vec<&IntegrationInfo> = all.iter().filter(|i| i.id.to_string() == raw || i.name == raw).collect();
    let i = match hits.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(ServiceError::NotFound {
                what: "integration",
                input: integration.to_owned(),
                suggestions: guildsmith_core::suggest::closest(raw, all.iter().map(|i| i.name.as_str()), 3),
            });
        }
        many => {
            return Err(ServiceError::Ambiguous {
                what: "integration",
                input: integration.to_owned(),
                candidates: many
                    .iter()
                    .map(|i| resolve::Candidate { name: i.name.clone(), id: i.id, parent: None })
                    .collect(),
            });
        }
    };
    let what = format!("{} integration {} ({})", i.kind, i.name, i.id);
    confirm::guarded(confirmed, vec![format!("remove the {what}; its roles and webhooks go with it")], || async {
        app.port.delete_integration(gid, i.id, &audit_reason(&format!("remove {what}"), reason)).await?;
        Ok(Deleted { deleted: what.clone() })
    })
    .await
}
