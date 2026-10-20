//! T031: `guildsmith moderation ban|purge|prune` in-process against `FakeDiscord`: previews,
//! exit 3 without `--yes` when there's no terminal, `--yes` doing it, and the confirm prompt.

use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::{Cli, Exit};
use guildsmith::dispatch_with;
use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;

struct Run {
    exit: Exit,
    out: String,
    err: String,
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

/// No terminal: nobody to ask.
async fn run(app: &Arc<App<FakeDiscord>>, args: &[&str]) -> Run {
    run_with(app, args, &mut |_| None).await
}

fn lab() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

#[tokio::test]
async fn ban_previews_and_exits_3_without_yes_on_a_non_terminal() {
    let app = lab();
    let r = run(&app, &["moderation", "ban", "-g", "1000", "--members", "sam,alex", "--delete-message-seconds", "1d"])
        .await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("Preview only; nothing changed. Would ban 2 target(s)"), "{}", r.err);
    assert!(r.err.contains("\"preview\":true"), "the preview's details are shown: {}", r.err);
    assert!(r.err.contains("<discord-data>"), "member names stay wrapped as untrusted: {}", r.err);
    assert!(r.err.contains("delete their messages from the last 86400 seconds"), "1d → 86400 s: {}", r.err);
    assert!(r.err.contains("not a terminal") && r.err.contains("--yes"), "{}", r.err);
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn yes_does_it_and_json_prints_the_outcome() {
    let app = lab();
    let r = run(&app, &["moderation", "ban", "-g", "1000", "--members", "sam", "--yes", "--json"]).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    let v: serde_json::Value = serde_json::from_str(&r.out).expect("JSON on stdout");
    assert_eq!(v["state"], "completed");
    assert_eq!(v["succeeded"][0]["name"], "sam");
    assert_eq!(app.port.writes().iter().map(|w| w.method).collect::<Vec<_>>(), ["ban"]);
}

#[tokio::test]
async fn a_terminal_shows_the_preview_and_asks() {
    let app = lab();
    let mut asked = Vec::new();
    let r = run_with(&app, &["moderation", "kick", "-g", "1000", "sam"], &mut |q| {
        asked.push(q.to_owned());
        Some(false)
    })
    .await;
    assert_eq!(r.exit, Exit::Refused);
    assert!(r.err.contains("Cancelled; nothing was changed."), "{}", r.err);
    assert_eq!(asked.len(), 1);
    assert_eq!(app.port.writes(), []);
    let r = run_with(&app, &["moderation", "kick", "-g", "1000", "sam"], &mut |_| Some(true)).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(r.out.contains("1 succeeded"), "{}", r.out);
}

#[tokio::test]
async fn purge_takes_the_channel_positionally_and_prune_previews_a_count() {
    let app = lab();
    let r = run(&app, &["moderation", "purge", "-g", "1000", "#general", "--author", "sam", "--count", "50"]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("Preview only"), "{}", r.err);
    let r = run(&app, &["moderation", "prune", "-g", "1000", "--days", "30"]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("remove 0 member(s) inactive for 30 day(s)"), "{}", r.err);
    // The contract's example puts purge under `messages`; it's found there too.
    let r = run(&app, &["messages", "purge", "-g", "1000", "#general", "--count", "5"]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
}

#[tokio::test]
async fn bad_input_is_explained() {
    let app = lab();
    let r = run(&app, &["moderation", "bann", "-g", "1000"]).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("member-ban"), "lists the group's tools: {}", r.err);
    let r = run(&app, &["moderation", "ban", "-g", "1000", "--member", "sam", "--frobnicate", "1"]).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("unknown option --frobnicate") && r.err.contains("--members"), "{}", r.err);
    let r = run(&app, &["moderation", "timeout", "-g", "1000", "--member", "sam", "--duration", "soon"]).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("--duration"), "{}", r.err);
    let r = run(&app, &["nonsense"]).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("unknown command `nonsense`"), "{}", r.err);
}

#[tokio::test]
async fn read_only_refuses_with_exit_3() {
    let app = Arc::new(App::new(fixtures::lab_fake(), Policy { read_only: true, ..Policy::default() }));
    let r = run(&app, &["moderation", "timeout", "-g", "1000", "--member", "sam", "--duration", "1h"]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("read-only"), "{}", r.err);
    assert_eq!(app.port.writes(), []);
}
