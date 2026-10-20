//! T110: `guildsmith raw <METHOD> <path>` in-process against `FakeDiscord`: `{guild}` replaced
//! by `-g`, `--query` and `--body`, a write previewing (exit 3) until `--yes`.

use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::{Cli, Exit};
use guildsmith::dispatch_with;
use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_core::raw::RawMethod;
use serde_json::json;

struct Run {
    exit: Exit,
    out: String,
    err: String,
}

async fn run(app: &Arc<App<FakeDiscord>>, args: &[&str]) -> Run {
    let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).expect("args parse");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = dispatch_with(&cli, app.clone(), &mut out, &mut err, &mut |_| None).await;
    Run { exit, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}

fn lab() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

#[tokio::test]
async fn get_with_guild_placeholder_and_query() {
    let app = lab();
    app.port.set_raw_response(RawMethod::Get, "guilds/1000/audit-logs", 200, json!({"audit_log_entries": []}));
    let r = run(
        &app,
        &[
            "raw",
            "GET",
            "guilds/{guild}/audit-logs",
            "-g",
            "1000",
            "--query",
            "limit=5",
            "--query",
            "action_type=20",
            "--json",
        ],
    )
    .await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    let v: serde_json::Value = serde_json::from_str(&r.out).expect("JSON on stdout");
    assert_eq!(v["status"], 200);
    let sent = app.port.raw_requests();
    assert_eq!(sent[0].path, "guilds/1000/audit-logs");
    assert_eq!(sent[0].query, [("action_type", "20"), ("limit", "5")].map(|(k, v)| (k.to_owned(), v.to_owned())));
}

#[tokio::test]
async fn writes_preview_until_yes() {
    let app = lab();
    app.port.set_raw_response(RawMethod::Patch, "guilds/1000", 200, json!({"id": "1000", "name": "renamed"}));
    let args = ["raw", "PATCH", "guilds/{guild}", "-g", "1000", "--body", r#"{"name":"renamed"}"#];
    let r = run(&app, &args).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("Preview only") && r.err.contains("PATCH guilds/1000"), "{}", r.err);
    assert!(r.err.contains("--yes"), "{}", r.err);
    assert_eq!(app.port.writes(), []);

    let r = run(&app, &[&args[..], &["--yes"]].concat()).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(r.out.contains("HTTP 200"), "{}", r.out);
    assert_eq!(app.port.writes().len(), 1);
}

#[tokio::test]
async fn refused_routes_exit_3() {
    let app = lab();
    let r = run(&app, &["raw", "POST", "oauth2/token", "--yes"]).await;
    assert_eq!(r.exit, Exit::Refused, "{}", r.err);
    assert!(r.err.contains("refused"), "{}", r.err);
    assert_eq!(app.port.raw_requests(), []);
}
