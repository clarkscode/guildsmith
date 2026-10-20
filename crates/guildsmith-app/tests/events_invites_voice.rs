//! T046: the events, invites and voice services against `FakeDiscord` (US3, research E4/E5).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use guildsmith_app::App;
use guildsmith_app::bulk::Hooks;
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FAKE_NOW, FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_app::ports::Clock;
use guildsmith_app::services::events::{self, EventInput, LocationInput, RecurrenceInput};
use guildsmith_app::services::invites::{self, InviteInput, TargetAction};
use guildsmith_app::services::voice::{self, StageAction, VoiceAction};
use guildsmith_core::Id;
use guildsmith_core::Permission;
use guildsmith_core::snapshot::LiveRole;

const G: Option<&str> = Some("1000");

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(FAKE_NOW).unwrap())
    }
}

fn app() -> Arc<App<FakeDiscord>> {
    let mut s = fixtures::rich();
    s.roles.push(LiveRole {
        id: Id(12),
        name: "Boss".to_owned(),
        color: 0,
        secondary_color: None,
        tertiary_color: None,
        hoist: false,
        mentionable: false,
        managed: false,
        permissions: [Permission::Administrator].into_iter().collect(),
        position: 4,
        tags: None,
    });
    Arc::new(App::with_clock(FakeDiscord::new(vec![(s, fixtures::lab_members())]), Policy::default(), Arc::new(Fixed)))
}

fn event(name: &str, start: &str) -> EventInput {
    EventInput {
        name: Some(name.to_owned()),
        start: Some(start.to_owned()),
        location: Some(LocationInput::Voice("Lobby".to_owned())),
        ..EventInput::default()
    }
}

#[tokio::test]
async fn times_without_a_zone_are_utc_and_shown_as_utc() {
    let app = app();
    let e = events::event_create(&app, G, &event("Game night", "2026-10-23T20:00"), None).await.unwrap();
    assert_eq!(e.start, "2026-10-23T20:00:00Z");
    assert_eq!(e.when, "2026-10-23 20:00 UTC");
    let with_tz = EventInput { tz: Some("-07:00".to_owned()), ..event("Pacific night", "2026-10-23T20:00") };
    let e = events::event_create(&app, G, &with_tz, None).await.unwrap();
    assert_eq!(e.when, "2026-10-24 03:00 UTC");
    let offset = events::event_create(&app, G, &event("Offset", "2026-10-23T20:00:00+02:00"), None).await.unwrap();
    assert_eq!(offset.start, "2026-10-23T18:00:00Z");
    let named = EventInput { tz: Some("America/Los_Angeles".to_owned()), ..event("x", "2026-10-23T20:00") };
    assert!(events::event_create(&app, G, &named, None).await.unwrap_err().to_string().contains("offset"));
    assert!(
        events::event_create(&app, G, &event("past", "2026-10-01T20:00"), None)
            .await
            .unwrap_err()
            .to_string()
            .contains("future")
    );
}

#[tokio::test]
async fn external_events_need_a_location_and_an_end_and_recurrence_has_limits() {
    let app = app();
    let ext = EventInput {
        location: Some(LocationInput::External("Central Park".to_owned())),
        ..event("Picnic", "2026-10-25T12:00")
    };
    let err = events::event_create(&app, G, &ext, None).await.unwrap_err();
    assert!(err.to_string().contains("needs an end"), "{err}");
    let ok = EventInput { end: Some("2026-10-25T15:00".to_owned()), ..ext };
    let e = events::event_create(&app, G, &ok, None).await.unwrap();
    assert_eq!(e.location, "external: Central Park");
    let wrong_kind =
        EventInput { location: Some(LocationInput::Stage("Lobby".to_owned())), ..event("x", "2026-10-25T12:00") };
    assert!(events::event_create(&app, G, &wrong_kind, None).await.is_err());

    let weekly = |interval, days: &[&str]| RecurrenceInput {
        frequency: "weekly".to_owned(),
        interval,
        weekday: days.iter().map(|d| (*d).to_owned()).collect(),
        n_weekday: None,
        month: None,
        month_day: None,
    };
    let r = EventInput { recurrence: Some(weekly(Some(2), &["friday"])), ..event("Biweekly", "2026-10-23T20:00") };
    let e = events::event_create(&app, G, &r, None).await.unwrap();
    assert_eq!(e.repeats.as_deref(), Some("every 2 weeks on friday"));
    for bad in [weekly(Some(3), &["friday"]), weekly(None, &["friday", "monday"])] {
        let r = EventInput { recurrence: Some(bad), ..event("x", "2026-10-23T20:00") };
        assert!(matches!(events::event_create(&app, G, &r, None).await, Err(ServiceError::Invalid(_))));
    }
    let daily_interval = RecurrenceInput { frequency: "daily".to_owned(), ..weekly(Some(2), &[]) };
    let r = EventInput { recurrence: Some(daily_interval), ..event("x", "2026-10-23T20:00") };
    assert!(events::event_create(&app, G, &r, None).await.unwrap_err().to_string().contains("only for weekly"));
}

#[tokio::test]
async fn at_most_100_scheduled_or_active_events_and_status_changes() {
    let app = app();
    for i in 0..100 {
        events::event_create(&app, G, &event(&format!("e{i}"), "2026-11-01T10:00"), None).await.unwrap();
    }
    let err = events::event_create(&app, G, &event("one more", "2026-11-01T10:00"), None).await.unwrap_err();
    assert!(err.to_string().contains("100"), "{err}");
    let started =
        events::event_update(&app, G, "e0", &EventInput::default(), Some(events::EventStatus::Active), false, None)
            .await
            .unwrap();
    let Outcome::Done(e) = started else { panic!() };
    assert_eq!(e.status, "active");
    let renamed = EventInput { name: Some("new".to_owned()), ..EventInput::default() };
    assert!(
        events::event_update(&app, G, "e0", &renamed, None, false, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("active event")
    );
    let cancel =
        events::event_update(&app, G, "e1", &EventInput::default(), Some(events::EventStatus::Canceled), false, None)
            .await
            .unwrap();
    assert!(cancel.is_preview());
    assert!(events::event_delete(&app, G, "e2", false, None).await.unwrap().is_preview());
    app.port.interested(
        fixtures::GUILD,
        events::events_list(&app, G).await.unwrap()[3].id,
        &[fixtures::SAM, fixtures::ALEX],
    );
    let listed = events::events_list(&app, G).await.unwrap();
    assert!(listed.iter().any(|e| e.interested == 2));
}

#[tokio::test]
async fn invites_limits_roles_below_the_bot_and_targets() {
    let app = app();
    for (age, uses) in [(604_801, 0), (0, 101)] {
        let i = InviteInput { max_age: Some(age), max_uses: Some(uses), ..InviteInput::default() };
        assert!(matches!(invites::invite_create(&app, G, "#general", &i, None).await, Err(ServiceError::Invalid(_))));
    }
    let ok = InviteInput {
        max_age: Some(604_800),
        max_uses: Some(10),
        roles: vec!["@Member".to_owned()],
        ..InviteInput::default()
    };
    let inv = invites::invite_create(&app, G, "#general", &ok, None).await.unwrap();
    assert_eq!((inv.max_uses, inv.roles.clone(), inv.state.as_str()), (10, vec!["@Member".to_owned()], "active"));
    assert!(inv.url.starts_with("https://discord.gg/"));
    let high = InviteInput { roles: vec!["@Boss".to_owned()], ..InviteInput::default() };
    let err = invites::invite_create(&app, G, "#general", &high, None).await.unwrap_err();
    assert!(matches!(&err, ServiceError::Refused(r) if r.kind == RefusalKind::Preflight), "{err:?}");

    let t = invites::invite_target_users(&app, G, &inv.url, TargetAction::Add, &["sam".to_owned()]).await.unwrap();
    assert_eq!(t.users, [fixtures::SAM]);
    let listed = invites::invites_list(&app, G, None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(invites::invite_revoke(&app, G, &inv.code, false, None).await.unwrap().is_preview());
    invites::invite_revoke(&app, G, &inv.code, true, None).await.unwrap();
    assert_eq!(invites::invites_list(&app, G, None).await.unwrap(), []);
}

#[tokio::test]
async fn voice_actions_report_members_not_in_voice_and_limits() {
    let app = app();
    app.port.join_voice(fixtures::GUILD, fixtures::SAM, fixtures::LOBBY);
    let stage = Id(105);
    let action = VoiceAction { move_to: Some("Town Hall".to_owned()), ..VoiceAction::default() };
    let out = voice::voice_members(&app, G, &["sam".to_owned(), "alex".to_owned()], &action, None, &Hooks::none())
        .await
        .unwrap();
    assert_eq!(out.succeeded.iter().map(|t| t.id).collect::<Vec<_>>(), [fixtures::SAM]);
    assert_eq!(out.failed[0].item.id, fixtures::ALEX);
    assert!(out.failed[0].reason.contains("not in a voice channel"));
    assert_eq!(voice::voice_state(&app, G, "sam").await.unwrap().channel.as_deref(), Some("Town Hall"));
    let many: Vec<String> = (0..26).map(|i| format!("m{i}")).collect();
    assert!(matches!(
        voice::voice_members(&app, G, &many, &action, None, &Hooks::none()).await,
        Err(ServiceError::Invalid(_))
    ));
    let mute = VoiceAction { mute: Some(true), ..VoiceAction::default() };
    voice::voice_members(&app, G, &["sam".to_owned()], &mute, None, &Hooks::none()).await.unwrap();
    assert!(voice::voice_state(&app, G, "sam").await.unwrap().mute);

    for bad in ["", &"t".repeat(121)] {
        assert!(voice::stage(&app, G, "Town Hall", StageAction::Start, Some(bad), false, false, None).await.is_err());
    }
    let Outcome::Done(Some(st)) =
        voice::stage(&app, G, "Town Hall", StageAction::Start, Some("Town Hall Q&A"), false, false, None)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(st.channel_id, stage);
    assert!(voice::stage(&app, G, "Town Hall", StageAction::End, None, false, false, None).await.unwrap().is_preview());
    voice::stage(&app, G, "Town Hall", StageAction::End, None, false, true, None).await.unwrap();
    assert!(voice::voice_channel_status(&app, G, "Lobby", &"x".repeat(501), None).await.is_err());
    assert_eq!(
        voice::voice_channel_status(&app, G, "Lobby", "Playing chess", None).await.unwrap().status.as_deref(),
        Some("Playing chess")
    );
    assert_eq!(voice::voice_channel_status(&app, G, "Lobby", "", None).await.unwrap().status, None);
}
