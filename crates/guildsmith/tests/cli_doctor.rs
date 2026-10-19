//! T086: `guildsmith doctor` exit codes and `guildsmith auth`.

mod common;

use std::cell::RefCell;
use std::sync::Arc;

use clap::Parser;
use guildsmith::cli::auth::{self, AuthCommand, SecretStore};
use guildsmith::cli::{Cli, Exit};
use guildsmith_app::App;
use guildsmith_app::config::TokenSource;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::secret::Secret;
use guildsmith_core::Permission;

async fn doctor(fake: FakeDiscord) -> (Exit, String) {
    let cli = Cli::try_parse_from(["guildsmith", "doctor", "-g", "1000"]).unwrap();
    let app = Arc::new(App::new(fake, Policy::default()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = guildsmith::dispatch(&cli, app, &mut out, &mut err).await;
    (exit, String::from_utf8(out).unwrap())
}

#[tokio::test]
async fn doctor_exits_0_when_everything_passes_and_3_otherwise() {
    let (exit, out) = doctor(fixtures::lab_fake()).await;
    assert_eq!(exit, Exit::Ok, "{out}");
    assert!(out.contains("All checks passed."), "{out}");

    let mut s = fixtures::lab();
    s.roles.iter_mut().find(|r| r.id == fixtures::BOT_ROLE).unwrap().permissions =
        [Permission::ManageChannels].into_iter().collect();
    let (exit, out) = doctor(FakeDiscord::new(vec![(s, fixtures::lab_members())])).await;
    assert_eq!(exit, Exit::Refused, "{out}");
    assert!(out.contains("[FAIL] permissions (core)") && out.contains("fix: "), "{out}");
}

#[derive(Default)]
struct MemoryStore(RefCell<Option<String>>);

impl SecretStore for MemoryStore {
    fn get(&self) -> Result<Option<Secret>, String> {
        Ok(self.0.borrow().as_deref().map(Secret::new))
    }

    fn set(&self, token: &Secret) -> Result<(), String> {
        *self.0.borrow_mut() = Some(token.expose().to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<bool, String> {
        Ok(self.0.borrow_mut().take().is_some())
    }
}

fn auth_run(cmd: AuthCommand, store: &MemoryStore, source: Option<TokenSource>, input: &str) -> (Exit, String) {
    let input = input.to_owned();
    let mut read = move || Ok(input.clone());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = auth::run(cmd, store, source, &mut read, &mut out, &mut err);
    (exit, String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap())
}

#[test]
fn auth_set_reads_stdin_never_an_argument() {
    assert!(Cli::try_parse_from(["guildsmith", "auth", "set", common::CANARY]).is_err(), "no token argument");
    let store = MemoryStore::default();
    let (exit, out) = auth_run(AuthCommand::Set, &store, None, &format!("{}\n", common::CANARY));
    assert_eq!(exit, Exit::Ok, "{out}");
    assert_eq!(store.0.borrow().as_deref(), Some(common::CANARY));
    assert!(!out.contains(common::CANARY), "{out}");
    let (exit, _) = auth_run(AuthCommand::Set, &store, None, "Bearer abc\n");
    assert_eq!(exit, Exit::Refused, "user tokens are refused");
}

#[test]
fn auth_status_never_prints_the_token() {
    let store = MemoryStore::default();
    store.set(&Secret::new(common::CANARY)).unwrap();
    for source in [Some(TokenSource::Keyring), Some(TokenSource::Env), Some(TokenSource::ConfigFile), None] {
        let (exit, out) = auth_run(AuthCommand::Status, &store, source, "");
        assert_eq!(exit, Exit::Ok);
        assert!(!out.contains(common::CANARY), "{out}");
    }
    let (_, out) = auth_run(AuthCommand::Status, &store, Some(TokenSource::Keyring), "");
    assert!(out.contains("stored in the OS keyring"), "{out}");
    let (exit, out) = auth_run(AuthCommand::Clear, &store, None, "");
    assert_eq!(exit, Exit::Ok);
    assert!(out.contains("Removed"), "{out}");
    assert!(store.0.borrow().is_none());
}

#[test]
fn doctor_with_the_real_binary_never_prints_the_token() {
    // No server reachable here: the token check fails cleanly, without the token in output.
    let out =
        common::run(&["doctor", "-g", "1000", "--log", "trace"], &[("DISCORD_TOKEN", "Bearer gs-canary-7f3a")], "");
    assert_eq!(out.code, Some(3), "{}", out.stderr);
    assert!(out.stdout.contains("not a bot token"), "{}", out.stdout);
    assert!(!out.stdout.contains(common::CANARY) && !out.stderr.contains(common::CANARY));
}
