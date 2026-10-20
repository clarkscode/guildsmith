//! T076: the settings service against `FakeDiscord` (US7, research E4).

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_app::services::settings::{self, NO_SERVER_CREATION, SettingsSection, TemplateAction};
use guildsmith_core::snapshot::{LiveChannelKind, LiveOverwrite, OverwriteKind};
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::spec::onboarding::{OnboardingSpec, OptionSpec, PromptSpec};
use guildsmith_core::spec::settings::{FeaturesSpec, SettingsSpec, VerificationLevel};
use guildsmith_core::spec::welcome::{WelcomeChannelSpec, WelcomeScreenSpec};
use guildsmith_core::{Id, Permission};

const G: Option<&str> = Some("1000");

/// The lab server plus 6 public text channels and 2 where @everyone can't send.
fn app_with(admin: bool, community: bool) -> Arc<App<FakeDiscord>> {
    let mut s = fixtures::lab();
    for i in 0..8u64 {
        let mut c = fixtures::channel(
            Id(300 + i),
            &format!("room-{i}"),
            LiveChannelKind::Known(ChannelKind::Text),
            None,
            10 + i32::try_from(i).unwrap(),
        );
        if i >= 6 {
            c.overwrites = vec![LiveOverwrite {
                id: fixtures::GUILD,
                kind: OverwriteKind::Role,
                allow: guildsmith_core::PermissionSet::EMPTY,
                deny: [Permission::SendMessages].into_iter().collect(),
            }];
        }
        s.channels.push(c);
    }
    if community {
        s.guild.features.insert("COMMUNITY".to_owned());
    }
    if !admin {
        s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap().permissions =
            [Permission::ManageGuild, Permission::ManageRoles, Permission::ManageChannels].into_iter().collect();
    }
    Arc::new(App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), Policy::default()))
}

fn onboarding(channels: &[&str]) -> OnboardingSpec {
    OnboardingSpec {
        enabled: true,
        default_channels: channels.iter().map(|c| (*c).to_owned()).collect(),
        prompts: vec![PromptSpec {
            title: "Which games do you play?".to_owned(),
            options: vec![OptionSpec {
                title: "Minecraft".to_owned(),
                roles: vec!["@Member".to_owned()],
                ..OptionSpec::default()
            }],
            in_onboarding: true,
            ..PromptSpec::default()
        }],
        ..OnboardingSpec::default()
    }
}

#[tokio::test]
async fn previews_show_old_to_new_and_change_nothing() {
    let app = app_with(true, false);
    let spec = SettingsSpec {
        description: Some("A lab for guildsmith".to_owned()),
        system_channel: Some("#general".to_owned()),
        verification_level: Some(VerificationLevel::Medium),
        afk_timeout: Some(900),
        ..SettingsSpec::default()
    };
    let Outcome::Preview(p) = settings::settings_update(&app, G, &spec, false, None).await.unwrap() else { panic!() };
    assert!(p.would.contains(&"description \"\" → \"A lab for guildsmith\"".to_owned()), "{:?}", p.would);
    assert!(p.would.contains(&"system_channel none → #general".to_owned()), "{:?}", p.would);
    assert!(p.would.contains(&"verification_level none → medium".to_owned()), "{:?}", p.would);
    assert!(p.would.contains(&"afk_timeout 300 → 900".to_owned()), "{:?}", p.would);
    assert_eq!(app.port.writes(), []);
    let Outcome::Done(done) = settings::settings_update(&app, G, &spec, true, None).await.unwrap() else { panic!() };
    assert_eq!(done.changes.len(), 4);
    let again = settings::settings_update(&app, G, &spec, false, None).await.unwrap();
    assert!(matches!(again, Outcome::Done(ref d) if d.changes.is_empty()), "nothing left to change: {again:?}");
    let view = settings::settings_get(&app, G, Some(SettingsSection::General)).await.unwrap();
    assert_eq!(view.general.unwrap().system_channel, "#general");
}

#[tokio::test]
async fn afk_timeout_is_one_of_five_values() {
    let app = app_with(true, false);
    for bad in [0, 120, 7200] {
        let spec = SettingsSpec { afk_timeout: Some(bad), ..SettingsSpec::default() };
        assert!(
            matches!(settings::settings_update(&app, G, &spec, true, None).await, Err(ServiceError::Invalid(_))),
            "{bad}"
        );
    }
    for ok in [60, 300, 900, 1800, 3600] {
        let spec = SettingsSpec { afk_timeout: Some(ok), ..SettingsSpec::default() };
        assert!(settings::settings_update(&app, G, &spec, true, None).await.is_ok(), "{ok}");
    }
}

#[tokio::test]
async fn community_and_discoverable_toggles_need_administrator() {
    let app = app_with(false, false);
    let spec = SettingsSpec {
        rules_channel: Some("#room-0".to_owned()),
        public_updates_channel: Some("#room-1".to_owned()),
        features: Some(FeaturesSpec { community: Some(true), ..FeaturesSpec::default() }),
        ..SettingsSpec::default()
    };
    let err = settings::settings_update(&app, G, &spec, true, None).await.unwrap_err();
    let ServiceError::Refused(r) = err else { panic!("{err:?}") };
    assert_eq!(r.kind, RefusalKind::Preflight);
    assert!(format!("{:?}", r.details).contains("administrator"), "{r:?}");
    // invites_disabled doesn't need it.
    let invites = SettingsSpec {
        features: Some(FeaturesSpec { invites_disabled: Some(true), ..FeaturesSpec::default() }),
        ..SettingsSpec::default()
    };
    assert!(settings::settings_update(&app, G, &invites, true, None).await.is_ok());
    // With Administrator, community needs the rules and updates channels.
    let admin = app_with(true, false);
    let bare = SettingsSpec {
        features: Some(FeaturesSpec { community: Some(true), ..FeaturesSpec::default() }),
        ..SettingsSpec::default()
    };
    assert!(
        settings::settings_update(&admin, G, &bare, true, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("rules_channel")
    );
    assert!(settings::settings_update(&admin, G, &spec, true, None).await.is_ok());
}

#[tokio::test]
async fn onboarding_needs_7_default_channels_with_5_where_everyone_can_send() {
    let app = app_with(true, true);
    let few = onboarding(&["#room-0", "#room-1", "#room-2", "#room-3", "#room-4", "#room-5"]);
    assert!(
        settings::onboarding_update(&app, G, &few, true, None).await.unwrap_err().to_string().contains("at least 7")
    );
    // 7 channels, but only 4 of them allow @everyone to send.
    let quiet = onboarding(&["#room-0", "#room-1", "#room-2", "#room-3", "#room-6", "#room-7", "#mod-log"]);
    let err = settings::onboarding_update(&app, G, &quiet, true, None).await.unwrap_err();
    assert!(err.to_string().contains("at least 5") && err.to_string().contains("4 of these"), "{err}");
    let ok = onboarding(&["#room-0", "#room-1", "#room-2", "#room-3", "#room-4", "#room-6", "#room-7"]);
    assert!(settings::onboarding_update(&app, G, &ok, false, None).await.unwrap().is_preview());
    let Outcome::Done(set) = settings::onboarding_update(&app, G, &ok, true, None).await.unwrap() else { panic!() };
    let first_id = set.prompts[0].id;
    assert!(first_id.is_some());
    // Unknown roles fail before anything changes; ids are kept when re-applied.
    let mut bad = ok.clone();
    bad.prompts[0].options[0].roles = vec!["@Nope".to_owned()];
    assert!(matches!(settings::onboarding_update(&app, G, &bad, true, None).await, Err(ServiceError::NotFound { .. })));
    let Outcome::Done(again) = settings::onboarding_update(&app, G, &ok, true, None).await.unwrap() else { panic!() };
    assert_eq!(again.prompts[0].id, first_id, "prompt matched by title keeps its id");
    // Not a community server: onboarding can't be enabled.
    let plain = app_with(true, false);
    let err = settings::onboarding_update(&plain, G, &ok, true, None).await.unwrap_err();
    assert!(format!("{err:?}").contains("COMMUNITY"), "{err:?}");
}

#[tokio::test]
async fn one_template_per_server_and_no_server_creation() {
    let app = app_with(true, false);
    let Outcome::Done(t) =
        settings::templates(&app, G, TemplateAction::Create, Some("Lab"), None, false).await.unwrap()
    else {
        panic!()
    };
    assert!(t.templates[0].url.starts_with("https://discord.new/"));
    let err = settings::templates(&app, G, TemplateAction::Create, Some("Again"), None, false).await.unwrap_err();
    assert!(err.to_string().contains("one template"), "{err}");
    assert!(settings::templates(&app, G, TemplateAction::Sync, None, None, false).await.is_ok());
    assert!(settings::templates(&app, G, TemplateAction::Delete, None, None, false).await.unwrap().is_preview());
    let err = settings::templates(&app, G, TemplateAction::Use, None, None, false).await.unwrap_err();
    let ServiceError::Refused(r) = err else { panic!() };
    assert_eq!(r.kind, RefusalKind::Unsupported);
    assert!(r.reason.contains(NO_SERVER_CREATION));
    assert!(r.hint.unwrap().contains("template_import"));
}

#[tokio::test]
async fn template_import_produces_a_valid_spec_and_never_overwrites() {
    let app = app_with(true, false);
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/../guildsmith-core/tests/fixtures/templates/gaming.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    app.port.add_public_template("hgM48av5Q69A", "Gaming", fixture["serialized_source_guild"].clone());
    let out = settings::template_import(&app, "https://discord.new/hgM48av5Q69A", None).await.unwrap();
    assert_eq!(out.template, "Gaming");
    let report = guildsmith_core::validate::validate_text(&out.toml);
    assert!(!report.has_errors(), "{:?}", report.problems);
    let path = std::env::temp_dir().join(format!("guildsmith-import-{}.toml", std::process::id()));
    std::fs::write(&path, "keep me").unwrap();
    assert!(settings::template_import(&app, "hgM48av5Q69A", Some(&path)).await.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn welcome_screen_and_integrations() {
    let app = app_with(true, true);
    let w = WelcomeScreenSpec {
        enabled: true,
        description: Some("Welcome!".to_owned()),
        channels: vec![WelcomeChannelSpec {
            channel: "#general".to_owned(),
            description: "Say hi".to_owned(),
            emoji: Some("👋".to_owned()),
        }],
    };
    assert!(settings::welcome_screen_update(&app, G, &w, None).await.unwrap().changed);
    assert!(!settings::welcome_screen_update(&app, G, &w, None).await.unwrap().changed);
    let six = WelcomeScreenSpec { channels: vec![w.channels[0].clone(); 6], ..w };
    assert!(matches!(settings::welcome_screen_update(&app, G, &six, None).await, Err(ServiceError::Invalid(_))));
    app.port.add_integration(fixtures::GUILD, Id(700), "Twitch Sub", "twitch");
    assert!(settings::integration_remove(&app, G, "Twitch Sub", false, None).await.unwrap().is_preview());
    settings::integration_remove(&app, G, "700", true, None).await.unwrap();
    assert_eq!(
        settings::settings_get(&app, G, Some(SettingsSection::Integrations)).await.unwrap().integrations.unwrap(),
        []
    );
}
