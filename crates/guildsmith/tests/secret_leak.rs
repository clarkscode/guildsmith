//! T039: the secret-leak harness (SC-006). Seeds the canary token and checks it appears in no
//! output, log or error. Each story adds its commands and tools here.

mod common;

use common::CANARY;
use guildsmith::logging::Scrubber;
use guildsmith_app::config::{Dirs, LoadOptions, MapEnv, NoStore, Os, load};
use guildsmith_app::ports::PortError;
use guildsmith_app::secret::Secret;

fn assert_clean(what: &str, text: &str) {
    assert!(!text.contains(CANARY), "{what} leaked the token:\n{text}");
}

#[test]
fn mcp_session_at_trace_never_shows_the_token() {
    let session = common::mcp_session(&[("no_such_tool", serde_json::json!({"token": "x"}))]);
    let out = common::run(&["mcp", "--log", "trace"], &[("DISCORD_TOKEN", CANARY)], &session);
    assert_clean("mcp stdout", &out.stdout);
    assert_clean("mcp stderr", &out.stderr);
}

#[test]
fn bad_config_errors_never_show_the_token() {
    let out = common::run(&["mcp"], &[("DISCORD_TOKEN", CANARY), ("GUILDSMITH_TOOLSETS", "core,bogus")], "");
    assert_eq!(out.code, Some(1));
    assert!(out.stderr.contains("unknown toolset `bogus`"), "{}", out.stderr);
    assert_clean("config error", &out.stderr);
    assert_clean("config error stdout", &out.stdout);
}

#[test]
fn config_and_errors_redact_the_token() {
    let env = MapEnv::new(&[("DISCORD_TOKEN", CANARY)]);
    let read = |_: &std::path::Path| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    let cfg = load(&LoadOptions {
        env: &env,
        config_path: None,
        dirs: Dirs { os: Os::Linux, home: None, appdata: None },
        store: &NoStore,
        read_file: &read,
    })
    .unwrap();
    assert_clean("Config Debug", &format!("{cfg:?}"));
    assert_clean("Secret Display", &Secret::new(CANARY).to_string());
    for e in [PortError::NoToken, PortError::NotBotToken, PortError::Unauthorized] {
        assert_clean("PortError", &e.to_string());
    }
}

#[test]
fn log_scrubber_removes_the_token_and_auth_headers() {
    let s = Scrubber::new(vec![CANARY.to_owned()]);
    assert_clean("scrubbed", &s.scrub(&format!("sending Authorization: Bot {CANARY}")));
    assert_clean("scrubbed", &s.scrub(&format!("{{\"token\":\"{CANARY}\"}}")));
}

#[tokio::test]
async fn us1_commands_never_show_the_token() {
    use clap::Parser;
    use guildsmith::cli::Cli;
    use guildsmith_app::App;
    use guildsmith_app::fake::{FakeDiscord, fixtures};
    use guildsmith_app::policy::Policy;

    let spec = std::env::temp_dir().join(format!("guildsmith-leak-{}.toml", std::process::id()));
    std::fs::write(&spec, format!("[guild]\nname = \"{CANARY}x\"\n[[roles]]\nnmae = \"x\"\n")).unwrap();
    let spec_path = spec.to_str().unwrap().to_owned();
    for args in [
        vec!["guilds"],
        vec!["guilds", "--json"],
        vec!["export", "-g", "1000"],
        vec!["export", "-g", "2000"],
        vec!["schema"],
    ] {
        let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).unwrap();
        let app = std::sync::Arc::new(App::new(
            FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())]),
            Policy::default(),
        ));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        guildsmith::dispatch(&cli, app, &mut out, &mut err).await;
        assert_clean(&format!("{args:?} stdout"), &String::from_utf8_lossy(&out));
        assert_clean(&format!("{args:?} stderr"), &String::from_utf8_lossy(&err));
    }
    let _ = (spec_path, std::fs::remove_file(&spec));
}
