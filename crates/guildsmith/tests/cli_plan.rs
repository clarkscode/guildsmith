//! T055: `plan` rendering (human and `--json`) and the `apply` gates, in-process against
//! `FakeDiscord`.

use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::{Cli, Exit};
use guildsmith::dispatch_with;
use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;

const SPEC: &str = r##"
[[roles]]
name = "Staff"
color = "#AA8800"
[[roles]]
name = "Moderator"
hoist = true

[[categories]]
name = "Community"
  [[categories.channels]]
  name = "general"
  kind = "text"
  topic = "Welcome!"
  [[categories.channels]]
  name = "staff-room"
  kind = "text"
  overwrites = [{ target = "@everyone", deny = ["view_channel"] }, { target = "@Staff", allow = ["view_channel"] }]
"##;

struct Run {
    exit: Exit,
    out: String,
    err: String,
}

fn spec_file(name: &str, text: &str) -> String {
    let path = std::env::temp_dir().join(format!("guildsmith-cli-plan-{}-{name}.toml", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

async fn run_with(
    app: &Arc<App<FakeDiscord>>,
    args: &[&str],
    confirm: &mut (dyn FnMut(&str) -> Option<bool> + Send),
) -> Run {
    let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).expect("args parse");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = dispatch_with(&cli, app.clone(), &mut out, &mut err, confirm).await;
    Run { exit, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}

async fn run(app: &Arc<App<FakeDiscord>>, args: &[&str]) -> Run {
    run_with(app, args, &mut |_| None).await
}

fn lab() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

#[tokio::test]
async fn plan_renders_for_people_and_as_json() {
    let app = lab();
    let file = spec_file("render", SPEC);
    let r = run(&app, &["plan", "-g", "1000", &file, "--json"]).await;
    let mut v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    // The hash covers the engine version, so it changes every release: redact it.
    let hash = v["plan_hash"].as_str().unwrap().to_owned();
    assert!(guildsmith_core::hash::is_plan_hash(&hash), "{hash}");
    v["plan_hash"] = "gsp_[hash]".into();
    insta::assert_snapshot!("plan_json", serde_json::to_string_pretty(&v).unwrap());

    let r = run(&app, &["plan", "-g", "1000", &file]).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    insta::assert_snapshot!("plan_human", r.out.replace(&hash, "gsp_[hash]"));
}

#[tokio::test]
async fn detailed_exitcode_is_0_without_changes_and_2_with() {
    let app = lab();
    let changes = spec_file("changes", SPEC);
    let r = run(&app, &["plan", "-g", "1000", &changes, "--detailed-exitcode"]).await;
    assert_eq!(r.exit, Exit::Changes);

    let same = spec_file("same", "[[roles]]\nname = \"Moderator\"\n");
    let r = run(&app, &["plan", "-g", "1000", &same, "--detailed-exitcode"]).await;
    assert_eq!(r.exit, Exit::Ok, "{}{}", r.out, r.err);
    assert!(r.out.starts_with("No changes."), "{}", r.out);
}

#[tokio::test]
async fn apply_without_yes_on_a_non_tty_exits_3() {
    let app = lab();
    let file = spec_file("nontty", SPEC);
    let r = run(&app, &["apply", "-g", "1000", &file]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("not a terminal"), "{}", r.err);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn yes_needs_a_plan_hash() {
    let app = lab();
    let file = spec_file("yes", SPEC);
    let r = run(&app, &["apply", "-g", "1000", &file, "--yes"]).await;
    assert_eq!(r.exit, Exit::Refused);
    assert!(r.err.contains("--yes needs --plan-hash from a reviewed plan"), "{}", r.err);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn a_mismatched_plan_hash_exits_3() {
    let app = lab();
    let file = spec_file("mismatch", SPEC);
    let r = run(&app, &["apply", "-g", "1000", &file, "--yes", "--plan-hash", "gsp_00000000000000000000000000000000"])
        .await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("server changed since plan"), "{}", r.err);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn reviewed_hash_with_yes_applies_and_converges() {
    let app = lab();
    let file = spec_file("ci", SPEC);
    let plan = run(&app, &["plan", "-g", "1000", &file, "--json"]).await;
    let hash = serde_json::from_str::<serde_json::Value>(&plan.out).unwrap()["plan_hash"].as_str().unwrap().to_owned();
    let r = run(&app, &["apply", "-g", "1000", &file, "--yes", "--plan-hash", &hash]).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(r.err.contains("Applied 5 change(s)."), "{}", r.err);
    let r = run(&app, &["plan", "-g", "1000", &file, "--detailed-exitcode"]).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.out);
}

#[tokio::test]
async fn interactive_apply_uses_the_shown_plans_hash() {
    // The user is shown the plan, then someone edits the server before they confirm.
    let app = lab();
    let file = spec_file("interactive", SPEC);
    let editor = app.clone();
    let mut confirm = move |_: &str| {
        editor.port.edit_guild(fixtures::GUILD, |s| {
            s.channels.iter_mut().find(|c| c.id == fixtures::LOBBY).unwrap().name = "Lounge".to_owned();
        });
        Some(true)
    };
    let r = run_with(&app, &["apply", "-g", "1000", &file], &mut confirm).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("Plan: 5 change(s)"), "the plan was shown first:\n{}", r.err);
    assert!(r.err.contains("server changed since plan"), "{}", r.err);
    assert_eq!(app.port.writes(), []);

    // Confirming an unchanged server applies.
    let r = run_with(&app, &["apply", "-g", "1000", &file], &mut |_| Some(true)).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    // Declining changes nothing.
    let more = spec_file("decline", &SPEC.replace("Welcome!", "Hello"));
    let writes = app.port.writes().len();
    let r = run_with(&app, &["apply", "-g", "1000", &more], &mut |_| Some(false)).await;
    assert_eq!(r.exit, Exit::Refused);
    assert_eq!(app.port.writes().len(), writes);
}

#[tokio::test]
async fn read_only_refuses_apply_before_any_request() {
    let app = Arc::new(App::new(fixtures::lab_fake(), Policy { read_only: true, ..Policy::default() }));
    let file = spec_file("ro", SPEC);
    let r = run(&app, &["apply", "-g", "1000", &file, "--yes", "--plan-hash", "gsp_00000000000000000000000000000000"])
        .await;
    assert_eq!(r.exit, Exit::Refused);
    assert!(r.err.contains("read-only"), "{}", r.err);
    assert_eq!(app.port.reads(), 0);
    let r = run(&app, &["plan", "-g", "1000", &file]).await;
    assert_eq!(r.exit, Exit::Ok, "plan is read-only and always allowed");
}
