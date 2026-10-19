//! T068: one server with several problems at once gives **one** report with all of them, and
//! nothing is written (FR-020, SC-009).

use guildsmith_app::App;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_app::preflight::{PreflightProblem, check_plan};
use guildsmith_app::services::apply::{self, Progress};
use guildsmith_app::services::plan;
use guildsmith_core::snapshot::{LiveRole, RoleTags, Snapshot};
use guildsmith_core::{Id, Permission, PermissionSet};

/// The lab server with a weakened bot: only `manage_channels`, below a human "Owner" role, plus
/// another bot's managed role.
fn troubled() -> Snapshot {
    let mut s = fixtures::lab();
    let bot = s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap();
    bot.permissions = [Permission::ManageChannels].into_iter().collect();
    let role = |id: u64, name: &str, position: i64, managed: bool| LiveRole {
        id: Id(id),
        name: name.to_owned(),
        color: 0,
        secondary_color: None,
        tertiary_color: None,
        hoist: false,
        mentionable: false,
        managed,
        permissions: PermissionSet::EMPTY,
        position,
        tags: managed.then_some(RoleTags { bot_id: Some(Id(77)), integration_id: None, premium_subscriber: false }),
    };
    s.roles.push(role(20, "Owner", 5, false));
    s.roles.push(role(21, "OtherBot", 1, true));
    s
}

const SPEC: &str = r##"
[[roles]]
name = "Owner"
color = "#FF0000"

[[roles]]
name = "OtherBot"
managed = true
hoist = true

[[categories]]
name = "Community"
  [[categories.channels]]
  name = "general"
  kind = "text"
  overwrites = [{ target = "@everyone", allow = ["ban_members"] }]
"##;

fn kinds(problems: &[PreflightProblem]) -> Vec<&'static str> {
    let mut v: Vec<&str> = problems
        .iter()
        .map(|p| match p {
            PreflightProblem::MissingPermission { .. } => "MissingPermission",
            PreflightProblem::RoleTooHigh { .. } => "RoleTooHigh",
            PreflightProblem::NotAdministrator { .. } => "NotAdministrator",
            PreflightProblem::CannotGrant { .. } => "CannotGrant",
            PreflightProblem::ManagedRoleEdit { .. } => "ManagedRoleEdit",
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[tokio::test]
async fn every_problem_is_reported_together_and_nothing_changes() {
    let app = App::new(FakeDiscord::new(vec![(troubled(), fixtures::lab_members())]), Policy::default());
    let p = plan::plan(&app, Some("1000"), SPEC).await.unwrap();
    let snapshot = app.port.current(fixtures::GUILD).unwrap();

    let problems = check_plan(&snapshot, &p.plan);
    assert_eq!(
        kinds(&problems),
        ["CannotGrant", "ManagedRoleEdit", "MissingPermission", "NotAdministrator", "RoleTooHigh"],
        "{problems:#?}"
    );
    assert!(problems.contains(&PreflightProblem::MissingPermission {
        perm: "manage_roles".to_owned(),
        needed_by: "update role @Owner: color none → #FF0000".to_owned(),
    }));
    assert!(
        problems
            .contains(&PreflightProblem::CannotGrant { perm: "ban_members".to_owned(), channel: "general".to_owned() })
    );
    assert!(
        problems
            .contains(&PreflightProblem::ManagedRoleEdit { role: "@OtherBot".to_owned(), field: "hoist".to_owned() })
    );
    for problem in &problems {
        assert_ne!(problem.fix(), "");
    }

    let mut sink = |_: &Progress| {};
    let err = apply::apply(&app, Some("1000"), SPEC, &p.plan_hash, &mut sink).await.unwrap_err();
    let ServiceError::Refused(r) = err else { panic!("{err:?}") };
    assert_eq!(r.kind, RefusalKind::Preflight);
    let listed = r.details.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), problems.len(), "the refusal lists every problem");
    assert!(r.reason.contains(&format!("preflight found {} problem(s)", problems.len())), "{}", r.reason);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn an_administrator_bot_passes() {
    let app = App::new(fixtures::lab_fake(), Policy::default());
    let p = plan::plan(&app, Some("1000"), "[[roles]]\nname = \"Member\"\nhoist = true\n").await.unwrap();
    let snapshot = app.port.current(fixtures::GUILD).unwrap();
    assert_eq!(check_plan(&snapshot, &p.plan), []);
}
