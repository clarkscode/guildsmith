//! `guildsmith auth set | clear | status`: the bot token in the OS keyring (FR-035).
//!
//! `set` reads the token from stdin or a hidden prompt, never from an argument, so it stays
//! out of shell history. `status` never prints the token.

use std::io::{self, Write};

use clap::{Args, Subcommand};
use guildsmith_app::config::TokenSource;
use guildsmith_app::secret::Secret;

use super::Exit;

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AuthCommand {
    /// Store the token in the OS keyring (read from stdin or a hidden prompt).
    Set,
    /// Remove the token from the OS keyring.
    Clear,
    /// Say where the token comes from, without showing it.
    Status,
}

/// Where `auth` keeps the token.
pub trait SecretStore {
    fn get(&self) -> Result<Option<Secret>, String>;
    fn set(&self, token: &Secret) -> Result<(), String>;
    /// `true` if a token was removed.
    fn clear(&self) -> Result<bool, String>;
}

/// The OS keyring (feature `keyring`).
pub struct Keyring;

#[cfg(feature = "keyring")]
impl SecretStore for Keyring {
    fn get(&self) -> Result<Option<Secret>, String> {
        guildsmith_app::config::TokenStore::get(&guildsmith_app::config::KeyringStore)
    }

    fn set(&self, token: &Secret) -> Result<(), String> {
        guildsmith_app::config::KeyringStore::set(token)
    }

    fn clear(&self) -> Result<bool, String> {
        guildsmith_app::config::KeyringStore::clear()
    }
}

#[cfg(not(feature = "keyring"))]
impl SecretStore for Keyring {
    fn get(&self) -> Result<Option<Secret>, String> {
        Ok(None)
    }

    fn set(&self, _: &Secret) -> Result<(), String> {
        Err("this build has no keyring support; set DISCORD_TOKEN instead".to_owned())
    }

    fn clear(&self) -> Result<bool, String> {
        Ok(false)
    }
}

/// Reads the token: a hidden prompt on a terminal, otherwise one line of stdin.
pub fn read_token() -> io::Result<String> {
    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        rpassword::prompt_password("Discord bot token (input hidden): ")
    } else {
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        Ok(line)
    }
}

/// Runs an `auth` command. `source` is where the loaded config found a token.
pub fn run(
    cmd: AuthCommand,
    store: &dyn SecretStore,
    source: Option<TokenSource>,
    read: &mut dyn FnMut() -> io::Result<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    match cmd {
        AuthCommand::Set => {
            let token = match read() {
                Ok(t) => t.trim().to_owned(),
                Err(e) => {
                    let _ = writeln!(err, "error: can't read the token: {e}");
                    return Exit::Error;
                }
            };
            if token.is_empty() {
                let _ = writeln!(err, "error: no token given; pipe it in or paste it at the prompt");
                return Exit::Error;
            }
            if token.starts_with("Bearer ") {
                let _ = writeln!(err, "refused: not a bot token; only bot tokens are supported");
                return Exit::Refused;
            }
            match store.set(&Secret::new(token.strip_prefix("Bot ").unwrap_or(&token))) {
                Ok(()) => {
                    let _ = writeln!(out, "Stored the bot token in the OS keyring.");
                    if source == Some(TokenSource::Env) {
                        let _ = writeln!(out, "note: DISCORD_TOKEN is set and takes precedence over the keyring.");
                    }
                    Exit::Ok
                }
                Err(e) => {
                    let _ = writeln!(err, "error: can't store the token: {e}");
                    Exit::Error
                }
            }
        }
        AuthCommand::Clear => match store.clear() {
            Ok(true) => {
                let _ = writeln!(out, "Removed the bot token from the OS keyring.");
                Exit::Ok
            }
            Ok(false) => {
                let _ = writeln!(out, "No token was stored in the OS keyring.");
                Exit::Ok
            }
            Err(e) => {
                let _ = writeln!(err, "error: can't remove the token: {e}");
                Exit::Error
            }
        },
        AuthCommand::Status => {
            let in_keyring = matches!(store.get(), Ok(Some(_)));
            let line = match source {
                Some(TokenSource::Env) => "a token is set in DISCORD_TOKEN (it takes precedence)",
                Some(TokenSource::Keyring) => "a token is stored in the OS keyring",
                Some(TokenSource::ConfigFile) => {
                    "a token is in the config file, in plain text; consider `guildsmith auth set`"
                }
                None => "no token configured; run `guildsmith auth set` or set DISCORD_TOKEN",
            };
            let _ = writeln!(out, "{line}");
            if in_keyring && source != Some(TokenSource::Keyring) {
                let _ = writeln!(out, "the OS keyring also holds a token");
            }
            let _ = writeln!(out, "run `guildsmith doctor -g <server>` to check that it works");
            Exit::Ok
        }
    }
}
