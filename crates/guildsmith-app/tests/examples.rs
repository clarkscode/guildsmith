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

/// T102: the full-server example (one item of every kind, files in `assets/`) applies to an
/// empty server from its own directory, as the CLI reads it, and then plans to nothing.
#[tokio::test]
async fn full_server_example_applies_and_converges() {
    use guildsmith_app::assets::Caller;

    let dir = std::path::PathBuf::from(format!("{}/../../examples/full-server", env!("CARGO_MANIFEST_DIR")));
    let text = std::fs::read_to_string(dir.join("guild.toml")).unwrap();
    let v = validate::validate(&text);
    assert!(v.valid && v.problems.is_empty(), "{:?}", v.problems);
    let caller = Caller::Cli { spec_dir: Some(&dir) };

    let app = App::new(FakeDiscord::new(vec![(fixtures::empty(Id(3000), "Empty"), vec![])]), Policy::default());
    let p = plan::plan_in(&app, Some("3000"), &text, caller).await.unwrap_or_else(|e| panic!("{e}"));
    let mut sink = |_: &Progress| {};
    let report = apply::apply_in(&app, Some("3000"), &text, &p.plan_hash, caller, &mut sink).await.unwrap();
    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.secrets.len(), 1, "the new webhook's URL, once");
    assert!(report.secrets[0].url.starts_with("https://discord.com/api/webhooks/"));
    let ids: Vec<String> = report.written_ids.iter().map(|w| format!("{}.{}", w.path, w.field)).collect();
    for want in [
        "automod[0].id",
        "emoji[0].id",
        "stickers[0].id",
        "sounds[0].id",
        "webhooks[0].id",
        "invites[0].code",
        "events[0].id",
        "messages[0].id",
    ] {
        assert!(ids.contains(&want.to_owned()), "{want} missing from {ids:?}");
    }

    let again = plan::plan_in(&app, Some("3000"), &text, caller).await.unwrap();
    let left: Vec<&str> = again.ops.iter().map(|o| o.summary.as_str()).collect();
    assert!(left.is_empty(), "full-server doesn't converge: {left:#?}");
}
