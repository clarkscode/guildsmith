//! T075: the snapshot cache (research R18): 60-second TTL per server, cleared by writes and
//! `refresh`, bypassed by `spec_apply` and preflight.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::ports::Clock;
use guildsmith_app::services::apply::{self, Progress};
use guildsmith_app::services::live::{self, RoleInput};
use guildsmith_app::services::{doctor, inspect, plan};

struct TestClock(Mutex<SystemTime>);

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

impl TestClock {
    fn advance(&self, secs: u64) {
        *self.0.lock().unwrap() += Duration::from_secs(secs);
    }
}

fn app() -> (App<FakeDiscord>, Arc<TestClock>) {
    let clock = Arc::new(TestClock(Mutex::new(SystemTime::UNIX_EPOCH + Duration::from_hours(500_000))));
    (App::with_clock(fixtures::lab_fake(), Policy::default(), clock.clone()), clock)
}

#[tokio::test]
async fn snapshots_are_cached_for_60_seconds_per_server() {
    let (app, clock) = app();
    inspect::overview(&app, Some("1000")).await.unwrap();
    let reads = app.port.reads();
    inspect::roles(&inspect::snapshot(&app, Some("1000")).await.unwrap(), &[]).unwrap();
    assert_eq!(app.port.reads(), reads, "served from the cache");
    clock.advance(59);
    inspect::overview(&app, Some("1000")).await.unwrap();
    assert_eq!(app.port.reads(), reads, "still fresh at 59 s");
    clock.advance(2);
    inspect::overview(&app, Some("1000")).await.unwrap();
    assert_eq!(app.port.reads(), reads + 1, "expired after 60 s");
}

#[tokio::test]
async fn writes_and_refresh_clear_it() {
    let (app, _) = app();
    inspect::overview(&app, Some("1000")).await.unwrap();
    let input = RoleInput { name: Some("Helper".to_owned()), ..RoleInput::default() };
    live::role_upsert(&app, Some("1000"), None, &input, None).await.unwrap();
    let reads = app.port.reads();
    let overview = inspect::overview(&app, Some("1000")).await.unwrap();
    assert_eq!(app.port.reads(), reads + 1, "the write cleared the cache");
    assert!(overview.roles.iter().any(|r| r.name == "Helper"), "and the new role shows up");

    let cleared = live::refresh(&app, Some("1000")).await.unwrap();
    assert_eq!(cleared.cleared, [fixtures::GUILD]);
    inspect::overview(&app, Some("1000")).await.unwrap();
    assert_eq!(app.port.reads(), reads + 2);
    assert_eq!(live::refresh(&app, None).await.unwrap().cleared, [fixtures::GUILD], "no guild clears everything");
}

#[tokio::test]
async fn spec_apply_and_preflight_bypass_it() {
    let (app, _) = app();
    let text = "[[roles]]\nname = \"Member\"\nhoist = true\n";
    let p = plan::plan(&app, Some("1000"), text).await.unwrap(); // fills the cache
    let reads = app.port.reads();
    doctor::bot_preflight(&app, Some("1000"), None, None).await.unwrap();
    assert_eq!(app.port.reads(), reads + 1, "preflight fetched fresh");
    let mut sink = |_: &Progress| {};
    apply::apply(&app, Some("1000"), text, &p.plan_hash, &mut sink).await.unwrap();
    assert!(app.port.reads() > reads + 1, "apply fetched fresh");
}
