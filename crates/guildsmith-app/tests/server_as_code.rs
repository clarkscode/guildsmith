//! US9 round trip (quickstart §4, SC-010/SC-011): a server built from the full-server example
//! is exported with its files, the export plans to nothing against the server it came from,
//! applies to an empty server, and then plans to nothing there too. The export holds no
//! webhook URL or token.

use std::path::{Path, PathBuf};

use guildsmith_app::App;
use guildsmith_app::assets::Caller;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::apply::{self, ApplyReport, Progress};
use guildsmith_app::services::{export, plan};
use guildsmith_core::Id;

fn example_dir() -> PathBuf {
    PathBuf::from(format!("{}/../../examples/full-server", env!("CARGO_MANIFEST_DIR")))
}

async fn apply_all(app: &App<FakeDiscord>, guild: &str, text: &str, dir: &Path) -> ApplyReport {
    let caller = Caller::Cli { spec_dir: Some(dir) };
    let p = plan::plan_in(app, Some(guild), text, caller).await.unwrap_or_else(|e| panic!("{e}"));
    let mut sink = |_: &Progress| {};
    let report = apply::apply_in(app, Some(guild), text, &p.plan_hash, caller, &mut sink).await.unwrap();
    assert!(report.is_complete(), "{report:#?}");
    report
}

async fn assert_converged(app: &App<FakeDiscord>, guild: &str, text: &str, dir: &Path) {
    let again = plan::plan_in(app, Some(guild), text, Caller::Cli { spec_dir: Some(dir) }).await.unwrap();
    let left: Vec<&str> = again.ops.iter().map(|o| o.summary.as_str()).collect();
    assert!(left.is_empty(), "server {guild} doesn't converge: {left:#?}");
}

#[tokio::test]
async fn export_apply_elsewhere_and_both_plans_are_empty() {
    let fake = FakeDiscord::new(vec![
        (fixtures::empty(Id(3000), "Lab"), vec![]),
        (fixtures::empty(Id(4000), "Empty"), vec![]),
    ]);
    let app = App::new(fake, Policy::default());
    let source = std::fs::read_to_string(example_dir().join("guild.toml")).unwrap();
    apply_all(&app, "3000", &source, &example_dir()).await;

    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("server_as_code_export");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let exported = export::export_to(&app, Some("3000"), "2026-10-20", Some(&out_dir)).await.unwrap();
    std::fs::write(out_dir.join("guild.toml"), &exported.toml).unwrap();
    for section in [
        "[guild.settings]",
        "[[automod]]",
        "[onboarding]",
        "[welcome_screen]",
        "[[emoji]]",
        "[[stickers]]",
        "[[sounds]]",
        "[[webhooks]]",
        "[[invites]]",
        "[[events]]",
    ] {
        assert!(exported.toml.contains(section), "{section} missing:\n{}", exported.toml);
    }
    assert!(
        !exported.toml.contains("/api/webhooks/") && !exported.toml.contains("fake-webhook-token"),
        "{}",
        exported.toml
    );
    assert!(out_dir.join("assets/emoji/guildsmith.png").exists(), "files are saved next to the spec");
    assert!(exported.toml.contains("file = \"assets/emoji/guildsmith.png\""), "{}", exported.toml);

    assert_converged(&app, "3000", &exported.toml, &out_dir).await;
    let report = apply_all(&app, "4000", &exported.toml, &out_dir).await;
    assert_eq!(report.secrets.len(), 1, "the copy's new webhook URL, shown once");
    assert_converged(&app, "4000", &exported.toml, &out_dir).await;
}
