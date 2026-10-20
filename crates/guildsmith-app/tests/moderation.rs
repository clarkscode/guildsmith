//! T023: the moderation service against `FakeDiscord` (US1, research E4/E10).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use guildsmith_app::App;
use guildsmith_app::bulk::{BulkState, Hooks};
use guildsmith_app::confirm::Outcome;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::moderation::ModerationState;
use guildsmith_app::fake::{FAKE_NOW, FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_app::ports::{AuditEntry, Clock, Member};
use guildsmith_app::services::moderation::{self, BulkResult};
use guildsmith_core::snapshot::LiveRole;
use guildsmith_core::{Id, Permission};

const G: Option<&str> = Some("1000");
const BOSS: Id = Id(60);
const BOSS_ROLE: Id = Id(12);

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(FAKE_NOW).unwrap())
    }
}

fn member(id: Id, name: &str, roles: &[Id]) -> Member {
    Member { id, username: name.to_owned(), display_name: None, nick: None, role_ids: roles.to_vec(), timed_out: false }
}

/// The lab server plus @Boss above the bot's role, held by `boss`.
fn app_with(policy: Policy) -> App<FakeDiscord> {
    let mut s = fixtures::lab();
    s.roles.push(LiveRole {
        id: BOSS_ROLE,
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
    let mut members = fixtures::lab_members();
    members.push(member(BOSS, "boss", &[BOSS_ROLE]));
    App::with_clock(FakeDiscord::new(vec![(s, members)]), policy, Arc::new(Fixed))
}

fn app() -> App<FakeDiscord> {
    app_with(Policy::default())
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn methods(app: &App<FakeDiscord>) -> Vec<&'static str> {
    app.port.writes().iter().map(|w| w.method).collect()
}

#[tokio::test]
async fn previews_change_nothing() {
    let app = app();
    let h = Hooks::none();
    let ban = moderation::member_ban(&app, G, &strs(&["sam", "alex"]), 0, false, None, &h).await.unwrap();
    let BulkResult::Preview(p) = ban else { panic!("preview expected") };
    assert_eq!((p.total, p.targets.len(), p.batch_size), (2, 2, 200));
    assert!(moderation::member_kick(&app, G, &strs(&["sam"]), false, None, &h).await.unwrap().is_preview());
    assert!(moderation::members_prune(&app, G, 30, &[], false, None).await.unwrap().is_preview());
    let lock = moderation::raid_lockdown(&app, G, Some(3600), None, false, false, None).await.unwrap();
    assert!(lock.is_preview());
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn the_owner_and_the_bot_are_never_targeted_and_higher_members_fail_while_the_rest_continue() {
    let app = app();
    let h = Hooks::none();
    let list = strs(&["owner", "guildsmith", "boss", "sam", "alex"]);
    let BulkResult::Preview(p) = moderation::member_kick(&app, G, &list, false, None, &h).await.unwrap() else {
        panic!()
    };
    let skipped: Vec<(String, String)> = p.problems.iter().map(|f| (f.item.name.clone(), f.reason.clone())).collect();
    assert_eq!(skipped.len(), 3, "{skipped:?}");
    assert!(skipped.iter().any(|(n, r)| n == "owner" && r.contains("owner")));
    assert!(skipped.iter().any(|(n, r)| n == "guildsmith" && r.contains("itself")));
    assert!(skipped.iter().any(|(n, r)| n == "boss" && r.contains("at or above the bot")));

    let BulkResult::Done(out) = moderation::member_kick(&app, G, &list, true, None, &h).await.unwrap() else {
        panic!()
    };
    assert_eq!(out.state, BulkState::Completed);
    assert_eq!(out.succeeded.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["sam", "Lexi"]);
    assert_eq!(out.failed.len(), 3);
    assert_eq!(out.total(), 5);
    assert_eq!(methods(&app), ["kick", "kick"], "skipped members cost no request");
    assert!(app.port.writes().iter().all(|w| w.reason.as_deref().unwrap().starts_with("guildsmith: kick ")));
}

#[tokio::test]
async fn a_per_item_failure_doesnt_stop_the_rest() {
    let app = app();
    app.port.fail_on("timeout", Some(fixtures::SAM));
    let out = moderation::member_timeout(&app, G, &strs(&["sam", "alex"]), 3600, None, &Hooks::none()).await;
    // A 500 for one member is Discord failing: the run stops there and reports the rest.
    let out = out.unwrap();
    assert_eq!(out.total(), 2);
    assert!(matches!(out.state, BulkState::Stopped { .. }));
    app.port.clear_failures();

    // A 403-style failure for one member keeps going.
    let app = self::app();
    let s = app.port.current(fixtures::GUILD).unwrap();
    assert!(s.bot.permissions.contains(Permission::ModerateMembers));
    let out =
        moderation::member_timeout(&app, G, &strs(&["sam", "alex", "boss"]), 60, None, &Hooks::none()).await.unwrap();
    assert_eq!(out.succeeded.len(), 2);
    assert_eq!(out.failed[0].item.id, BOSS);
    assert_eq!(out.state, BulkState::Completed);
}

#[tokio::test]
async fn bans_of_450_users_go_in_3_batches() {
    let app = app();
    let ids: Vec<String> = (1_000_000u64..1_000_450).map(|i| i.to_string()).collect();
    let progress = AtomicUsize::new(0);
    let on_progress = |_| {
        progress.fetch_add(1, Ordering::SeqCst);
    };
    let hooks = Hooks { progress: &on_progress, cancelled: &|| false };
    let BulkResult::Preview(p) = moderation::member_ban(&app, G, &ids, 3600, false, None, &hooks).await.unwrap() else {
        panic!()
    };
    assert_eq!((p.total, p.targets.len(), p.more, p.estimated_requests), (450, 50, 400, 3));
    let BulkResult::Done(out) = moderation::member_ban(&app, G, &ids, 3600, true, Some("raid"), &hooks).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(out.succeeded.len(), 450, "{}", out.summary());
    assert_eq!(methods(&app), ["bulk_ban", "bulk_ban", "bulk_ban"]);
    assert_eq!(progress.load(Ordering::SeqCst), 3);
    assert!(app.port.writes()[0].reason.as_deref().unwrap().contains("(raid)"));

    // A single member uses the plain ban endpoint.
    let app = self::app();
    moderation::member_ban(&app, G, &strs(&["sam"]), 0, true, None, &Hooks::none()).await.unwrap();
    assert_eq!(methods(&app), ["ban"]);
    let bans = moderation::bans_list(&app, G, None, None, None).await.unwrap();
    assert_eq!(bans.bans.len(), 1);
}

#[tokio::test]
async fn bulk_ban_needs_manage_guild_too_and_cancels_between_batches() {
    let mut s = fixtures::lab();
    s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap().permissions =
        [Permission::BanMembers].into_iter().collect();
    let app = App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), Policy::default());
    let err =
        moderation::member_ban(&app, G, &strs(&["sam", "alex"]), 0, true, None, &Hooks::none()).await.unwrap_err();
    let ServiceError::Refused(r) = err else { panic!("{err:?}") };
    assert_eq!(r.kind, RefusalKind::Preflight);
    assert!(r.reason.contains("manage_guild") || format!("{:?}", r.details).contains("manage_guild"), "{r:?}");

    let app = self::app();
    let ids: Vec<String> = (2_000_000u64..2_000_450).map(|i| i.to_string()).collect();
    let batches = AtomicUsize::new(0);
    let on_progress = |_| {
        batches.fetch_add(1, Ordering::SeqCst);
    };
    let cancelled = || batches.load(Ordering::SeqCst) >= 1;
    let hooks = Hooks { progress: &on_progress, cancelled: &cancelled };
    let BulkResult::Done(out) = moderation::member_ban(&app, G, &ids, 0, true, None, &hooks).await.unwrap() else {
        panic!()
    };
    assert_eq!(out.state, BulkState::Cancelled);
    assert_eq!((out.succeeded.len(), out.not_attempted.len()), (200, 250));
}

#[tokio::test]
async fn timeouts_are_limited_to_28_days_and_skip_administrators() {
    let app = app();
    let h = Hooks::none();
    assert!(matches!(
        moderation::member_timeout(&app, G, &strs(&["sam"]), 29 * 86_400, None, &h).await,
        Err(ServiceError::Invalid(_))
    ));
    let out = moderation::member_timeout(&app, G, &strs(&["sam"]), 28 * 86_400, None, &h).await.unwrap();
    assert_eq!(out.succeeded.len(), 1);
    let cleared = moderation::member_timeout(&app, G, &strs(&["sam"]), 0, None, &h).await.unwrap();
    assert_eq!(cleared.succeeded.len(), 1);
    assert!(app.port.writes()[1].reason.as_deref().unwrap().contains("clear timeout"));
}

#[tokio::test]
async fn prune_days_are_1_to_30_and_the_preview_is_discords_count() {
    let app = app();
    for days in [0u8, 31] {
        assert!(matches!(
            moderation::members_prune(&app, G, days, &[], false, None).await,
            Err(ServiceError::Invalid(_))
        ));
    }
    app.port.set_inactive(fixtures::GUILD, &[fixtures::SAM, fixtures::ALEX]);
    // Sam has @Member, Alex has @Moderator: neither has no roles.
    let Outcome::Preview(p) = moderation::members_prune(&app, G, 30, &[], false, None).await.unwrap() else { panic!() };
    assert!(p.would[0].contains("remove 0 member(s)"), "{:?}", p.would);
    let Outcome::Preview(p) = moderation::members_prune(&app, G, 30, &strs(&["@Member"]), false, None).await.unwrap()
    else {
        panic!()
    };
    assert!(p.would[0].contains("remove 1 member(s)") && p.would[0].contains("@Member"), "{:?}", p.would);
    let Outcome::Done(done) = moderation::members_prune(&app, G, 30, &strs(&["@Member"]), true, None).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(done.pruned, Some(1));
    assert_eq!(methods(&app), ["prune"]);
}

#[tokio::test]
async fn purge_splits_out_messages_older_than_14_days() {
    let app = app();
    let sam = fixtures::lab_members().into_iter().find(|m| m.id == fixtures::SAM).unwrap();
    let alex = fixtures::lab_members().into_iter().find(|m| m.id == fixtures::ALEX).unwrap();
    let old = app.port.post_at(fixtures::GUILD, fixtures::GENERAL, &sam, "old spam", "2026-10-01T12:00:00Z");
    for i in 0..3 {
        app.port.post_at(fixtures::GUILD, fixtures::GENERAL, &sam, &format!("spam {i}"), "2026-10-19T11:00:00Z");
    }
    app.port.post_at(fixtures::GUILD, fixtures::GENERAL, &alex, "hello", "2026-10-19T11:30:00Z");
    let h = Hooks::none();

    let BulkResult::Preview(p) =
        moderation::messages_purge(&app, G, "#general", Some("sam"), None, None, None, false, false, None, &h)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!((p.total, p.targets.len(), p.problems.len()), (4, 3, 1));
    assert_eq!(p.problems[0].item.id, old);
    assert!(p.notes.iter().any(|n| n.contains("include_old")), "{:?}", p.notes);

    let BulkResult::Done(out) =
        moderation::messages_purge(&app, G, "#general", Some("sam"), None, None, None, false, true, None, &h)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!((out.succeeded.len(), out.failed.len()), (3, 1));
    assert!(out.failed[0].reason.contains("14 days"));
    assert_eq!(methods(&app), ["bulk_delete"]);

    let BulkResult::Done(out) =
        moderation::messages_purge(&app, G, "#general", Some("sam"), None, None, None, true, true, None, &h)
            .await
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(out.succeeded.iter().map(|m| m.id).collect::<Vec<_>>(), [old]);
    assert_eq!(methods(&app), ["bulk_delete", "delete_message"]);
    // Alex's message is untouched.
    let left = guildsmith_app::services::live::messages_read(&app, G, "#general", None, None).await.unwrap();
    assert_eq!(left.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), ["hello"]);
}

#[tokio::test]
async fn raid_lockdown_lasts_at_most_24_hours() {
    let app = app();
    for (inv, dms) in [(Some(86_401), None), (None, Some(0)), (None, None)] {
        assert!(matches!(
            moderation::raid_lockdown(&app, G, inv, dms, false, true, None).await,
            Err(ServiceError::Invalid(_))
        ));
    }
    let Outcome::Done(i) =
        moderation::raid_lockdown(&app, G, Some(86_400), Some(3600), false, true, None).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(i.invites_disabled_until.as_deref(), Some("2026-10-20T12:00:00Z"));
    assert_eq!(i.dms_disabled_until.as_deref(), Some("2026-10-19T13:00:00Z"));
    let Outcome::Done(i) = moderation::raid_lockdown(&app, G, None, None, true, true, None).await.unwrap() else {
        panic!()
    };
    assert_eq!(i.invites_disabled_until, None);
}

#[tokio::test]
async fn bulk_roles_skip_members_who_already_match_and_unbans_need_ids() {
    let app = app();
    let out = moderation::roles_bulk_assign(&app, G, "@Member", &strs(&["sam", "alex"]), &[], None, &Hooks::none())
        .await
        .unwrap();
    assert_eq!(out.succeeded.len(), 2);
    assert_eq!(methods(&app), ["add_member_role"], "sam already has @Member");
    assert!(matches!(
        moderation::roles_bulk_assign(&app, G, "@Boss", &strs(&["sam"]), &[], None, &Hooks::none()).await,
        Err(ServiceError::Refused(_))
    ));
    assert!(moderation::ban_remove(&app, G, &strs(&["sam"]), None, &Hooks::none()).await.is_err());
    let out = moderation::ban_remove(&app, G, &strs(&["51", "<@52>"]), None, &Hooks::none()).await.unwrap();
    assert_eq!(out.succeeded.len(), 2);
}

#[tokio::test]
async fn read_only_refuses_every_write_before_any_request() {
    let app = app_with(Policy { read_only: true, ..Policy::default() });
    let h = Hooks::none();
    let reads = app.port.reads();
    for err in [
        moderation::member_ban(&app, G, &strs(&["sam"]), 0, true, None, &h).await.map(|_| ()).unwrap_err(),
        moderation::member_kick(&app, G, &strs(&["sam"]), true, None, &h).await.map(|_| ()).unwrap_err(),
        moderation::member_timeout(&app, G, &strs(&["sam"]), 60, None, &h).await.map(|_| ()).unwrap_err(),
        moderation::members_prune(&app, G, 7, &[], true, None).await.map(|_| ()).unwrap_err(),
        moderation::raid_lockdown(&app, G, Some(60), None, false, true, None).await.map(|_| ()).unwrap_err(),
        moderation::ban_remove(&app, G, &strs(&["51"]), None, &h).await.map(|_| ()).unwrap_err(),
    ] {
        assert!(matches!(&err, ServiceError::Refused(r) if r.kind == RefusalKind::ReadOnly), "{err:?}");
    }
    assert_eq!(app.port.reads(), reads, "refused before any request");
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn the_audit_log_filters_by_action_and_user() {
    let app = app();
    app.port.edit_res::<ModerationState>(fixtures::GUILD, |m| {
        m.audit = vec![
            AuditEntry {
                id: Id(1),
                action_type: 22,
                user_id: Some(fixtures::ALEX),
                target_id: Some(Id(9)),
                reason: Some("spam".to_owned()),
                changes: vec![],
            },
            AuditEntry {
                id: Id(2),
                action_type: 20,
                user_id: Some(fixtures::OWNER),
                target_id: Some(Id(8)),
                reason: None,
                changes: vec![],
            },
        ];
    });
    let all = moderation::audit_log(&app, G, None, None, None, None).await.unwrap();
    assert_eq!(all.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), ["member_kick", "member_ban_add"]);
    let bans = moderation::audit_log(&app, G, Some("member_ban_add"), Some("alex"), None, None).await.unwrap();
    assert_eq!(bans.len(), 1);
    assert_eq!(bans[0].reason.as_deref(), Some("spam"));
    let err = moderation::audit_log(&app, G, Some("member_ban"), None, None, None).await.unwrap_err();
    assert!(err.to_string().contains("member_ban_add"), "{err}");
}

/// SC-008: a 50-account ban list, from preview to final report, well under 2 minutes. With the
/// fake the time is the planning cost; against Discord it is one bulk-ban request.
#[tokio::test]
async fn sc008_fifty_bans_are_one_request_and_fast() {
    let app = app();
    let ids: Vec<String> = (2_000_000u64..2_000_050).map(|i| i.to_string()).collect();
    let started = std::time::Instant::now();
    let BulkResult::Preview(p) = moderation::member_ban(&app, G, &ids, 0, false, None, &Hooks::none()).await.unwrap()
    else {
        panic!()
    };
    assert_eq!((p.total, p.estimated_requests), (50, 1));
    let BulkResult::Done(out) = moderation::member_ban(&app, G, &ids, 0, true, None, &Hooks::none()).await.unwrap()
    else {
        panic!()
    };
    let elapsed = started.elapsed();
    assert_eq!(out.succeeded.len(), 50);
    assert_eq!(methods(&app), ["bulk_ban"]);
    assert!(elapsed.as_secs() < 120, "{elapsed:?}");
    eprintln!("SC-008: 50 bans, 1 request, {elapsed:?} (FakeDiscord)");
}
