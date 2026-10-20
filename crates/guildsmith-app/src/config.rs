//! Configuration loading (FR-034, research R14).
//!
//! Each setting comes from the first source that has it: env vars, then the `--config` file
//! (or, without `--config`, the first default file that exists). The token is looked up in
//! `DISCORD_TOKEN`, then the OS keyring, then the config file (with a plain-text warning).

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use guildsmith_core::Id;
use serde::Deserialize;

use crate::policy::{self, Policy, Toolset};
use crate::secret::Secret;
use crate::toolsets;

/// Reads environment variables. Lets tests supply their own.
pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
}

/// The process environment.
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// A fixed set of variables, for tests.
pub struct MapEnv(HashMap<String, String>);

impl MapEnv {
    #[must_use]
    pub fn new(vars: &[(&str, &str)]) -> Self {
        MapEnv(vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect())
    }
}

impl Env for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

/// Where the token can be stored outside the config file.
pub trait TokenStore {
    fn get(&self) -> Result<Option<Secret>, String>;
}

/// No secure store (keyring feature off, or tests).
pub struct NoStore;

impl TokenStore for NoStore {
    fn get(&self) -> Result<Option<Secret>, String> {
        Ok(None)
    }
}

/// The OS keyring (`guildsmith auth set` writes here).
#[cfg(feature = "keyring")]
pub struct KeyringStore;

#[cfg(feature = "keyring")]
impl KeyringStore {
    const SERVICE: &'static str = "guildsmith";
    const USER: &'static str = "discord-bot-token";

    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(Self::SERVICE, Self::USER).map_err(|e| e.to_string())
    }

    pub fn set(token: &Secret) -> Result<(), String> {
        Self::entry()?.set_password(token.expose()).map_err(|e| e.to_string())
    }

    pub fn clear() -> Result<bool, String> {
        match Self::entry()?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(feature = "keyring")]
impl TokenStore for KeyringStore {
    fn get(&self) -> Result<Option<Secret>, String> {
        match Self::entry()?.get_password() {
            Ok(t) => Ok(Some(Secret::new(t))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The platform, for picking default config paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Directories used for default config paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub os: Os,
    pub home: Option<PathBuf>,
    /// `%APPDATA%` on Windows.
    pub appdata: Option<PathBuf>,
}

impl Dirs {
    #[must_use]
    pub fn from_env(env: &dyn Env) -> Self {
        let os = Os::current();
        let home = env.var("HOME").or_else(|| env.var("USERPROFILE")).map(PathBuf::from);
        Dirs { os, home, appdata: env.var("APPDATA").map(PathBuf::from) }
    }
}

/// Default config file locations, most preferred first (contracts/cli.md).
#[must_use]
pub fn default_config_paths(dirs: &Dirs) -> Vec<PathBuf> {
    let xdg = dirs.home.as_ref().map(|h| h.join(".config").join("guildsmith").join("config.toml"));
    match dirs.os {
        Os::Windows => dirs.appdata.iter().map(|a| a.join("guildsmith").join("config.toml")).collect(),
        Os::MacOs => dirs
            .home
            .iter()
            .map(|h| h.join("Library").join("Application Support").join("guildsmith").join("config.toml"))
            .chain(xdg)
            .collect(),
        Os::Linux => xdg.into_iter().collect(),
    }
}

/// Where the token came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Env,
    Keyring,
    ConfigFile,
}

/// Loaded configuration. `Debug` never shows the token.
#[derive(Debug, Clone)]
pub struct Config {
    pub token: Option<Secret>,
    pub token_source: Option<TokenSource>,
    pub allowlist: Option<BTreeSet<Id>>,
    pub read_only: bool,
    pub toolsets: BTreeSet<Toolset>,
    pub blocked: BTreeSet<Toolset>,
    pub file_roots: Vec<PathBuf>,
    pub log: Option<String>,
    /// The file settings were read from, if any.
    pub file: Option<PathBuf>,
    /// Problems worth telling the user about (printed to stderr at startup).
    pub warnings: Vec<String>,
}

impl Config {
    #[must_use]
    pub fn policy(&self) -> Policy {
        Policy {
            allowlist: self.allowlist.clone(),
            read_only: self.read_only,
            toolsets: self.toolsets.difference(&self.blocked).copied().collect(),
            blocked: self.blocked.clone(),
            file_roots: self.file_roots.clone(),
        }
    }
}

/// What `load` reads from.
pub struct LoadOptions<'a> {
    pub env: &'a dyn Env,
    /// `--config <path>`. When set, the file must exist.
    pub config_path: Option<PathBuf>,
    pub dirs: Dirs,
    pub store: &'a dyn TokenStore,
    pub read_file: &'a dyn Fn(&Path) -> io::Result<String>,
}

/// Config could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("can't read config file {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid config file {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    token: Option<String>,
    guilds: Option<Vec<String>>,
    read_only: Option<bool>,
    toolsets: Option<Vec<String>>,
    toolsets_block: Option<Vec<String>>,
    file_roots: Option<Vec<PathBuf>>,
    log: Option<String>,
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn read_file_config(opts: &LoadOptions<'_>) -> Result<(FileConfig, Option<PathBuf>), ConfigError> {
    let candidates = match &opts.config_path {
        Some(p) => vec![p.clone()],
        None => default_config_paths(&opts.dirs),
    };
    for path in candidates {
        match (opts.read_file)(&path) {
            Ok(text) => {
                let cfg = toml::from_str::<FileConfig>(&text)
                    .map_err(|e| ConfigError::Parse { path: path.clone(), message: e.message().to_owned() })?;
                return Ok((cfg, Some(path)));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && opts.config_path.is_none() => {}
            Err(source) => return Err(ConfigError::Read { path, source }),
        }
    }
    Ok((FileConfig::default(), None))
}

/// Loads configuration from env, file and token store.
pub fn load(opts: &LoadOptions<'_>) -> Result<Config, ConfigError> {
    let (file, file_path) = read_file_config(opts)?;
    let mut warnings = Vec::new();

    let (token, token_source) = if let Some(t) = opts.env.var("DISCORD_TOKEN").filter(|t| !t.trim().is_empty()) {
        (Some(Secret::new(t.trim())), Some(TokenSource::Env))
    } else {
        let stored = opts.store.get().unwrap_or_else(|e| {
            warnings.push(format!("couldn't read the OS keyring: {e}"));
            None
        });
        if let Some(t) = stored {
            (Some(t), Some(TokenSource::Keyring))
        } else if let Some(t) = file.token.as_deref().filter(|t| !t.trim().is_empty()) {
            warnings.push(
                "the bot token is stored in plain text in the config file; \
                 consider `guildsmith auth set` to keep it in the OS keyring"
                    .to_owned(),
            );
            (Some(Secret::new(t.trim())), Some(TokenSource::ConfigFile))
        } else {
            (None, None)
        }
    };

    let allowlist = match opts.env.var("GUILDSMITH_GUILDS") {
        Some(v) => Some(policy::parse_allowlist(&v).map_err(ConfigError::Invalid)?),
        None => match &file.guilds {
            Some(list) => Some(policy::parse_allowlist(&list.join(",")).map_err(ConfigError::Invalid)?),
            None => None,
        },
    }
    .filter(|set| !set.is_empty());

    let read_only = opts.env.var("GUILDSMITH_READ_ONLY").map_or(file.read_only.unwrap_or(false), |v| truthy(&v));

    let toolsets_raw = opts.env.var("GUILDSMITH_TOOLSETS").or_else(|| file.toolsets.as_ref().map(|t| t.join(",")));
    let toolsets = policy::parse_toolsets(toolsets_raw.as_deref().unwrap_or("default"))
        .map_err(|e| ConfigError::Invalid(e.to_string()))?;

    let block_raw =
        opts.env.var("GUILDSMITH_TOOLSETS_BLOCK").or_else(|| file.toolsets_block.as_ref().map(|t| t.join(",")));
    let blocked = toolsets::parse_blocked(block_raw.as_deref().unwrap_or(""))
        .map_err(|e| ConfigError::Invalid(format!("GUILDSMITH_TOOLSETS_BLOCK: {e}")))?;
    for t in toolsets.intersection(&blocked) {
        warnings.push(format!("the {t} toolset is both enabled and blocked; the block wins"));
    }

    let file_roots = match opts.env.var("GUILDSMITH_FILE_ROOTS") {
        Some(v) => std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect(),
        None => file.file_roots.unwrap_or_default(),
    };

    let log = opts.env.var("GUILDSMITH_LOG").or(file.log);

    Ok(Config {
        token,
        token_source,
        allowlist,
        read_only,
        toolsets,
        blocked,
        file_roots,
        log,
        file: file_path,
        warnings,
    })
}
