//! T043: CLI tests for `export`, `validate`, `schema` and `guilds`, in-process against
//! `FakeDiscord`.

use std::collections::BTreeSet;
use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::{Cli, Exit};
use guildsmith::dispatch;
use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_core::Id;

struct Run {
    exit: Exit,
    out: String,
    err: String,
}

async fn run(args: &[&str], policy: Policy) -> Run {
    let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).expect("args parse");
    let app = Arc::new(App::new(FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())]), policy));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = dispatch(&cli, app, &mut out, &mut err).await;
    Run { exit, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("guildsmith-cli-{}-{name}", std::process::id()))
}

#[tokio::test]
async fn export_prints_toml_and_refuses_to_overwrite() {
    let r = run(&["export", "-g", "1000"], Policy::default()).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(r.out.starts_with("#:schema "), "{}", r.out);

    let path = temp("export.toml");
    std::fs::write(&path, "existing").unwrap();
    let p = path.to_str().unwrap();
    let r = run(&["export", "-g", "1000", "-o", p], Policy::default()).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("refusing to overwrite"), "{}", r.err);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");

    let r = run(&["export", "-g", "1000", "-o", p, "--force"], Policy::default()).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(std::fs::read_to_string(&path).unwrap().contains("[guild]"));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn validate_prints_file_line_col_and_exits_1() {
    let path = temp("bad.toml");
    std::fs::write(&path, "[[channels]]\nname = \"v\"\nkind = \"voice\"\nuser_limit = 100\n").unwrap();
    let p = path.to_str().unwrap();
    let r = run(&["validate", p], Policy::default()).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.out.contains(&format!("{p}:4:14: error: user_limit must be 0–99")), "{}", r.out);

    std::fs::write(&path, "[[roles]]\nname = \"ok\"\n").unwrap();
    let r = run(&["validate", p], Policy::default()).await;
    assert_eq!(r.exit, Exit::Ok, "{}", r.out);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn schema_prints_json_schema_2020_12() {
    let r = run(&["schema"], Policy::default()).await;
    assert_eq!(r.exit, Exit::Ok);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(r.out, guildsmith_core::spec::json_schema_text());
}

#[tokio::test]
async fn servers_outside_the_allowlist_exit_3() {
    let policy = Policy { allowlist: Some(BTreeSet::from([Id(999)])), ..Policy::default() };
    let r = run(&["export", "-g", "1000"], policy).await;
    assert_eq!(r.exit, Exit::Refused);
    assert!(r.err.contains("not on the allowlist"), "{}", r.err);
}

#[tokio::test]
async fn removed_bot_exits_1_with_a_clear_message() {
    let r = run(&["export", "-g", "2000"], Policy::default()).await;
    assert_eq!(r.exit, Exit::Error);
    assert!(r.err.contains("bot is not in this server"), "{}", r.err);
}

#[tokio::test]
async fn guilds_lists_allowed_servers() {
    let r = run(&["guilds"], Policy::default()).await;
    assert_eq!(r.exit, Exit::Ok);
    assert!(r.out.contains("1000") && r.out.contains("Guildsmith Lab"), "{}", r.out);
    let r = run(&["guilds", "--json"], Policy::default()).await;
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["guilds"][0]["allowed"], true);
}
