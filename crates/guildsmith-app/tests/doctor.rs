//! T085: `DoctorService` (FR-033).

use std::collections::BTreeSet;

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, Toolset};
use guildsmith_app::preflight::PreflightProblem;
use guildsmith_app::services::doctor::{DoctorReport, Status, doctor};
use guildsmith_app::services::inspect;
use guildsmith_core::{Id, Permission, PermissionSet};

fn allowed() -> Policy {
    Policy {
        allowlist: Some(BTreeSet::from([fixtures::GUILD])),
        toolsets: BTreeSet::from([Toolset::Core, Toolset::Moderation]),
        ..Policy::default()
    }
}

fn status(r: &DoctorReport, name: &str) -> Status {
    r.checks.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no check {name}: {r:#?}")).status
}

#[tokio::test]
async fn a_correct_setup_passes_every_check() {
    let app = App::new(fixtures::lab_fake(), allowed());
    let r = doctor(&app, Some("1000")).await;
    assert!(r.ok, "{r:#?}");
    assert!(r.checks.iter().all(|c| c.status == Status::Pass), "{r:#?}");
    assert!(r.checks.iter().any(|c| c.name == "allowlist" && c.detail.contains("1000")));
}

#[tokio::test]
async fn an_invalid_token_is_reported_without_the_token() {
    let fake = fixtures::lab_fake();
    fake.set_invalid_token();
    let r = doctor(&App::new(fake, allowed()), Some("1000")).await;
    assert!(!r.ok);
    assert_eq!(status(&r, "token"), Status::Fail);
    let token = r.checks.iter().find(|c| c.name == "token").unwrap();
    assert!(token.detail.contains("token is invalid"), "{}", token.detail);
    assert_eq!(status(&r, "permissions"), Status::Skipped);
}

#[tokio::test]
async fn a_user_token_is_not_a_bot_token_and_every_command_refuses() {
    let fake = fixtures::lab_fake();
    fake.set_user_account();
    let app = App::new(fake, allowed());
    let r = doctor(&app, Some("1000")).await;
    assert!(r.checks.iter().any(|c| c.name == "token" && c.detail.contains("not a bot token")), "{r:#?}");
    let err = inspect::overview(&app, Some("1000")).await.unwrap_err();
    assert!(err.is_refusal(), "{err:?}");
    assert!(err.to_string().contains("not a bot token"), "{err}");
}

#[tokio::test]
async fn missing_permissions_name_the_toolset_and_role_order_is_checked() {
    let mut s = fixtures::lab();
    // The bot keeps only manage_channels and sits below a new "Owner" role.
    s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap().permissions =
        [Permission::ManageChannels].into_iter().collect();
    let mut owner = s.roles.iter().find(|r| r.id == fixtures::MODERATOR).unwrap().clone();
    (owner.id, owner.name, owner.position, owner.permissions) = (Id(20), "Owner".to_owned(), 9, PermissionSet::EMPTY);
    s.roles.push(owner);
    let app = App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), allowed());
    let r = doctor(&app, Some("1000")).await;
    assert!(!r.ok);
    let core = r.checks.iter().find(|c| c.name == "permissions (core)").unwrap();
    assert!(core.problems.iter().any(|p| matches!(&p.problem,
        PreflightProblem::MissingPermission { perm, needed_by } if perm == "manage_roles" && needed_by.ends_with("in the core toolset"))), "{core:#?}");
    assert!(core.problems.iter().any(|p| matches!(p.problem, PreflightProblem::NotAdministrator { .. })));
    let moderation = r.checks.iter().find(|c| c.name == "permissions (moderation)").unwrap();
    assert!(
        moderation.problems.iter().any(|p| matches!(&p.problem,
        PreflightProblem::MissingPermission { perm, .. } if perm == "ban_members")),
        "{moderation:#?}"
    );
    let order = r.checks.iter().find(|c| c.name == "role order").unwrap();
    assert_eq!(order.status, Status::Fail);
    assert!(order.problems.iter().any(|p| p.message.contains("@Owner")), "{order:#?}");
    assert!(order.problems.iter().all(|p| !p.fix.is_empty()));
}

#[tokio::test]
async fn the_allowlist_status_is_reported() {
    let open = App::new(fixtures::lab_fake(), Policy::default());
    let r = doctor(&open, Some("1000")).await;
    assert_eq!(status(&r, "allowlist"), Status::Warn);
    assert!(r.ok, "a missing allowlist warns but doesn't fail: {r:#?}");
    let closed =
        App::new(fixtures::lab_fake(), Policy { allowlist: Some(BTreeSet::from([Id(999)])), ..Policy::default() });
    let r = doctor(&closed, Some("1000")).await;
    assert_eq!(status(&r, "server"), Status::Fail, "{r:#?}");
}

/// 002 T020: per-group permissions, the `MESSAGE_CONTENT` intent for `messages`, and prune
/// needing Administrator when the server has `PRUNE_REQUIRES_ADMIN`.
#[tokio::test]
async fn groups_report_their_permissions_and_intents() {
    let policy = Policy { toolsets: BTreeSet::from([Toolset::Core, Toolset::Messages, Toolset::Polls]), ..allowed() };
    let fake = fixtures::lab_fake();
    fake.set_app_flags(0);
    let r = doctor(&App::new(fake, policy.clone()), Some("1000")).await;
    assert_eq!(status(&r, "intents"), Status::Fail, "{r:#?}");
    let c = r.checks.iter().find(|c| c.name == "intents").unwrap();
    assert!(
        matches!(&c.problems[0].problem, PreflightProblem::MissingIntent { intent, .. } if intent == "MESSAGE_CONTENT")
    );
    assert!(c.problems[0].fix.contains("developer portal"));
    assert!(r.checks.iter().any(|c| c.name == "permissions (polls)"), "one check per registered group");

    let fake = fixtures::lab_fake();
    fake.set_app_flags(guildsmith_app::preflight::MESSAGE_CONTENT_FLAGS);
    let r = doctor(&App::new(fake, policy), Some("1000")).await;
    assert_eq!(status(&r, "intents"), Status::Pass);
    // No group that needs intents: no intents check at all.
    let r = doctor(&App::new(fixtures::lab_fake(), allowed()), Some("1000")).await;
    assert!(r.checks.iter().all(|c| c.name != "intents"));
}

#[tokio::test]
async fn prune_needs_administrator_when_the_server_says_so() {
    let mut s = fixtures::lab();
    s.guild.features.insert(guildsmith_app::preflight::PRUNE_REQUIRES_ADMIN.to_owned());
    s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap().permissions =
        [Permission::KickMembers].into_iter().collect::<PermissionSet>();
    let app = App::new(FakeDiscord::new(vec![(s, fixtures::lab_members())]), allowed());
    let snap = app.fresh_snapshot(fixtures::GUILD).await.unwrap();
    let problems = guildsmith_app::preflight::check_tools(&snap, ["members_prune"]);
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, PreflightProblem::MissingPermission { perm, .. } if perm == "administrator")),
        "{problems:?}"
    );
    let r =
        guildsmith_app::services::doctor::bot_preflight(&app, Some("1000"), Some("members_prune"), None).await.unwrap();
    assert!(!r.ok);
}
