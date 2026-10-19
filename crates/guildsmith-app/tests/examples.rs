//! T066: every shipped example validates, applies to an empty server, and then plans to
//! nothing (SC-002).

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::apply::{self, Progress};
use guildsmith_app::services::{plan, validate};
use guildsmith_core::Id;

const EXAMPLES: &[&str] = &["community", "dev-team", "gaming"];

fn read(name: &str) -> String {
    let path = format!("{}/../../examples/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[tokio::test]
async fn examples_validate_apply_and_converge() {
    for name in EXAMPLES {
        let text = read(name);
        let v = validate::validate(&text);
        assert!(v.valid && v.problems.is_empty(), "{name}: {:?}", v.problems);

        let app = App::new(FakeDiscord::new(vec![(fixtures::empty(Id(3000), "Empty"), vec![])]), Policy::default());
        let p = plan::plan(&app, Some("3000"), &text).await.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(p.has_changes());
        assert_eq!(p.risk, guildsmith_core::planner::Risk::Safe, "{name}: applying to an empty server is safe");
        let mut sink = |_: &Progress| {};
        let report = apply::apply(&app, Some("3000"), &text, &p.plan_hash, &mut sink).await.unwrap();
        assert!(report.is_complete(), "{name}: {report:?}");

        let again = plan::plan(&app, Some("3000"), &text).await.unwrap();
        let left: Vec<&str> = again.ops.iter().map(|o| o.summary.as_str()).collect();
        assert!(left.is_empty(), "{name} doesn't converge: {left:?}");
    }
}
