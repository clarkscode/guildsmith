//! T015: config precedence, per-OS default paths, token lookup order, `Secret` redaction.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use guildsmith_app::config::{Dirs, LoadOptions, MapEnv, Os, TokenSource, TokenStore, default_config_paths, load};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;

struct FakeStore(Option<&'static str>);

impl TokenStore for FakeStore {
    fn get(&self) -> Result<Option<Secret>, String> {
        Ok(self.0.map(Secret::new))
    }
}

fn files(entries: &[(&str, &str)]) -> HashMap<PathBuf, String> {
    entries.iter().map(|(p, c)| (PathBuf::from(p), (*c).to_owned())).collect()
}

fn linux() -> Dirs {
    Dirs { os: Os::Linux, home: Some("/home/u".into()), appdata: None }
}

fn run(
    env: &[(&str, &str)],
    config_path: Option<&str>,
    fs: &HashMap<PathBuf, String>,
    store: &dyn TokenStore,
) -> guildsmith_app::config::Config {
    let env = MapEnv::new(env);
    let read = |p: &Path| fs.get(p).cloned().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound));
    load(&LoadOptions {
        env: &env,
        config_path: config_path.map(PathBuf::from),
        dirs: linux(),
        store,
        read_file: &read,
    })
    .expect("config loads")
}

#[test]
fn default_paths_per_os() {
    assert_eq!(default_config_paths(&linux()), vec![PathBuf::from("/home/u/.config/guildsmith/config.toml")]);
    let win =
        Dirs { os: Os::Windows, home: Some("C:/Users/u".into()), appdata: Some("C:/Users/u/AppData/Roaming".into()) };
    assert_eq!(default_config_paths(&win), vec![PathBuf::from("C:/Users/u/AppData/Roaming/guildsmith/config.toml")]);
    let mac = Dirs { os: Os::MacOs, home: Some("/Users/u".into()), appdata: None };
    assert_eq!(
        default_config_paths(&mac),
        vec![
            PathBuf::from("/Users/u/Library/Application Support/guildsmith/config.toml"),
            PathBuf::from("/Users/u/.config/guildsmith/config.toml"),
        ]
    );
}

#[test]
fn env_beats_explicit_file_beats_default_file() {
    let fs = files(&[
        ("/home/u/.config/guildsmith/config.toml", "guilds = [\"1\"]\nlog = \"warn\"\nread_only = true\n"),
        ("/tmp/explicit.toml", "guilds = [\"2\"]\n"),
    ]);
    // Default file only.
    let c = run(&[], None, &fs, &FakeStore(None));
    assert_eq!(c.allowlist, Some([Id(1)].into()));
    assert!(c.read_only);
    // An explicit --config replaces the default file.
    let c = run(&[], Some("/tmp/explicit.toml"), &fs, &FakeStore(None));
    assert_eq!(c.allowlist, Some([Id(2)].into()));
    assert!(!c.read_only);
    // Env vars win over any file.
    let c = run(
        &[("GUILDSMITH_GUILDS", "3,4"), ("GUILDSMITH_LOG", "debug")],
        Some("/tmp/explicit.toml"),
        &fs,
        &FakeStore(None),
    );
    assert_eq!(c.allowlist, Some([Id(3), Id(4)].into()));
    assert_eq!(c.log.as_deref(), Some("debug"));
}

#[test]
fn missing_explicit_config_is_an_error() {
    let env = MapEnv::new(&[]);
    let read = |_: &Path| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    let err = load(&LoadOptions {
        env: &env,
        config_path: Some("/nope.toml".into()),
        dirs: linux(),
        store: &FakeStore(None),
        read_file: &read,
    })
    .unwrap_err();
    assert!(err.to_string().contains("/nope.toml"), "{err}");
}

#[test]
fn token_lookup_order_env_keyring_file() {
    let fs = files(&[("/home/u/.config/guildsmith/config.toml", "token = \"from-file\"\n")]);
    let c = run(&[("DISCORD_TOKEN", "from-env")], None, &fs, &FakeStore(Some("from-keyring")));
    assert_eq!(c.token.as_ref().unwrap().expose(), "from-env");
    assert_eq!(c.token_source, Some(TokenSource::Env));

    let c = run(&[], None, &fs, &FakeStore(Some("from-keyring")));
    assert_eq!(c.token.as_ref().unwrap().expose(), "from-keyring");
    assert_eq!(c.token_source, Some(TokenSource::Keyring));
    assert_eq!(c.warnings, Vec::<String>::new());

    let c = run(&[], None, &fs, &FakeStore(None));
    assert_eq!(c.token.as_ref().unwrap().expose(), "from-file");
    assert_eq!(c.token_source, Some(TokenSource::ConfigFile));
    assert!(c.warnings.iter().any(|w| w.contains("plain text")), "{:?}", c.warnings);

    let c = run(&[], None, &HashMap::new(), &FakeStore(None));
    assert!(c.token.is_none());
}

#[test]
fn secret_never_shows_its_value() {
    let s = Secret::new("gs-canary-7f3a");
    assert!(!format!("{s:?}").contains("gs-canary"));
    assert!(!format!("{s}").contains("gs-canary"));
    assert_eq!(s.expose(), "gs-canary-7f3a");
    let c = run(&[("DISCORD_TOKEN", "gs-canary-7f3a")], None, &HashMap::new(), &FakeStore(None));
    assert!(!format!("{c:?}").contains("gs-canary"), "Config's Debug must redact the token");
}
