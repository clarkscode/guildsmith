//! T054: `ApplyService` against `FakeDiscord`: audit reasons, the plan-hash gate, partial
//! applies that resume, and fresh snapshots.

use guildsmith_app::App;
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::{Policy, RefusalKind, audit_reason};
use guildsmith_app::services::apply::{self, ApplyReport, Progress};
use guildsmith_app::services::plan;
use guildsmith_core::Id;

/// Six single-request ops and no reorder: three role updates and three channel updates.
const SIX: &str = r##"
[[roles]]
name = "Moderator"
color = "#112233"
[[roles]]
name = "Member"
hoist = true
[[roles]]
name = "@everyone"
permissions = ["view_channel"]

[[categories]]
name = "Community"
  [[categories.channels]]
  name = "general"
  kind = "text"
  topic = "one"
  [[categories.channels]]
  name = "mod-log"
  kind = "text"
  topic = "two"

[[channels]]
name = "Lobby"
kind = "voice"
user_limit = 5
"##;

fn app() -> App<FakeDiscord> {
    App::new(fixtures::lab_fake(), Policy::default())
}

async fn run(app: &App<FakeDiscord>, text: &str, hash: &str) -> (Result<ApplyReport, ServiceError>, Vec<Progress>) {
    let mut events = Vec::new();
    let res = apply::apply(app, Some("1000"), text, hash, &mut |p: &Progress| events.push(p.clone())).await;
    (res, events)
}

fn refusal_kind(res: &Result<ApplyReport, ServiceError>) -> Option<RefusalKind> {
    match res {
        Err(ServiceError::Refused(r)) => Some(r.kind),
        _ => None,
    }
}

#[tokio::test]
async fn applies_with_audit_reasons_and_converges() {
    let app = app();
    let p = plan::plan(&app, Some("1000"), SIX).await.unwrap();
    assert_eq!(p.ops.len(), 6, "{:?}", p.ops);
    let (res, events) = run(&app, SIX, &p.plan_hash).await;
    let report = res.unwrap();
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.applied.len(), 6);
    assert_eq!(events.len(), 6, "progress for every op");
    assert_eq!((events[0].index, events[0].total), (1, 6));

    let writes = app.port.writes();
    assert_eq!(writes.len(), 6);
    for (w, summary) in writes.iter().zip(&report.applied) {
        let reason = w.reason.as_deref().unwrap();
        assert_eq!(reason, format!("guildsmith: {summary}"));
        assert!(reason.chars().count() <= 512);
    }
    assert!(!plan::plan(&app, Some("1000"), SIX).await.unwrap().has_changes(), "plan again: no changes");
}

#[test]
fn audit_reasons_are_truncated_to_512_characters() {
    let long = "x".repeat(600);
    let reason = audit_reason(&long, None);
    assert!(reason.starts_with("guildsmith: x"));
    assert_eq!(reason.chars().count(), 512);
}

#[tokio::test]
async fn a_made_up_hash_is_refused_and_nothing_changes() {
    let app = app();
    for hash in ["gsp_00000000000000000000000000000000", "not-a-hash"] {
        let (res, events) = run(&app, SIX, hash).await;
        assert_eq!(refusal_kind(&res), Some(RefusalKind::PlanMismatch), "{res:?}");
        assert_eq!(events, []);
    }
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn a_server_edited_between_plan_and_apply_is_refused() {
    let app = app();
    let p = plan::plan(&app, Some("1000"), SIX).await.unwrap(); // also fills the cache
    app.port.edit_guild(fixtures::GUILD, |s| {
        s.channels.iter_mut().find(|c| c.id == fixtures::LOBBY).unwrap().name = "Lounge".to_owned();
    });
    let reads = app.port.reads();
    let (res, _) = run(&app, SIX, &p.plan_hash).await;
    assert_eq!(refusal_kind(&res), Some(RefusalKind::PlanMismatch), "{res:?}");
    let Err(ServiceError::Refused(r)) = res else { unreachable!() };
    assert!(r.reason.contains("server changed since plan"), "{}", r.reason);
    assert_eq!(app.port.writes(), []);
    assert!(app.port.reads() > reads, "apply fetched a fresh snapshot instead of using the cache");
}

#[tokio::test]
async fn a_failure_stops_apply_and_the_next_plan_has_only_the_rest() {
    let app = app();
    let p = plan::plan(&app, Some("1000"), SIX).await.unwrap();
    let summaries: Vec<String> = p.ops.iter().map(|o| o.summary.clone()).collect();
    app.port.fail_write_at(3);
    let (res, _) = run(&app, SIX, &p.plan_hash).await;
    let report = res.unwrap();
    assert_eq!(report.applied, summaries[..2]);
    assert_eq!(report.failed.as_ref().unwrap().summary, summaries[2]);
    assert_eq!(report.skipped, summaries[3..]);
    assert_eq!(app.port.writes().len(), 2, "no retries");

    let again = plan::plan(&app, Some("1000"), SIX).await.unwrap();
    let left: Vec<String> = again.ops.iter().map(|o| o.summary.clone()).collect();
    assert_eq!(left, summaries[2..], "exactly the ops that didn't run");
}

#[tokio::test]
async fn creates_resolve_new_ids_for_later_ops() {
    let app = App::new(FakeDiscord::new(vec![(fixtures::empty(Id(3000), "Empty"), vec![])]), Policy::default());
    let text = r#"
[[roles]]
name = "Staff"
[[roles]]
name = "Member"
[[categories]]
name = "Info"
  [[categories.channels]]
  name = "staff-room"
  kind = "text"
  overwrites = [{ target = "@everyone", deny = ["view_channel"] }, { target = "@Staff", allow = ["view_channel"] }]
  [[categories.channels]]
  name = "rules"
  kind = "text"
"#;
    let p = plan::plan(&app, Some("3000"), text).await.unwrap();
    let mut sink = |_: &Progress| {};
    let report = apply::apply(&app, Some("3000"), text, &p.plan_hash, &mut sink).await.unwrap();
    assert!(report.is_complete(), "{report:?}");
    let staff = report.created["role:Staff"];
    let room = report.created["channel:Info/staff-room"];
    let s = app.port.current(Id(3000)).unwrap();
    let ch = s.channel(room).unwrap();
    assert_eq!(ch.parent_id, Some(report.created["category:Info"]));
    assert!(ch.overwrites.iter().any(|o| o.id == staff));
    let after = plan::plan(&app, Some("3000"), text).await.unwrap();
    assert!(!after.has_changes(), "{:?}", after.ops);
}
