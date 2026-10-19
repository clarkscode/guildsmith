//! The `guildsmith` binary: CLI commands plus the `mcp` subcommand, wired to the twilight
//! adapter. Kept as a library so commands can be tested in-process.

pub mod cli;
pub mod logging;
pub mod render;

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use guildsmith_app::App;
use guildsmith_app::config::{self, Config, Dirs, LoadOptions, ProcessEnv, TokenStore};
use guildsmith_app::policy;
use guildsmith_app::ports::DiscordPort;
use guildsmith_discord::TwilightDiscord;

use cli::{Cli, Command, Exit};

fn token_store() -> Box<dyn TokenStore> {
    #[cfg(feature = "keyring")]
    {
        Box::new(config::KeyringStore)
    }
    #[cfg(not(feature = "keyring"))]
    {
        Box::new(config::NoStore)
    }
}

/// Loads config and applies the global CLI flags on top.
pub fn load_config(cli: &Cli) -> Result<Config, String> {
    let env = ProcessEnv;
    let store = token_store();
    let read = |p: &Path| std::fs::read_to_string(p);
    let mut cfg = config::load(&LoadOptions {
        env: &env,
        config_path: cli.config.clone(),
        dirs: Dirs::from_env(&env),
        store: store.as_ref(),
        read_file: &read,
    })
    .map_err(|e| e.to_string())?;
    if cli.read_only {
        cfg.read_only = true;
    }
    if let Some(log) = &cli.log {
        cfg.log = Some(log.clone());
    }
    if let Command::Mcp(args) = &cli.command
        && let Some(list) = &args.toolsets
    {
        cfg.toolsets = policy::parse_toolsets(list).map_err(|e| e.to_string())?;
    }
    Ok(cfg)
}

/// The process entry point used by `main`.
#[must_use]
pub fn main_entry() -> std::process::ExitCode {
    let cli = Cli::parse();
    let cfg = match load_config(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return Exit::Error.into();
        }
    };
    logging::init(cfg.log.as_deref().unwrap_or("warn"), cfg.token.iter().map(|t| t.expose().to_owned()).collect());
    for w in &cfg.warnings {
        tracing::warn!("{w}");
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: can't start the async runtime: {e}");
            return Exit::Error.into();
        }
    };
    let app = Arc::new(App::new(TwilightDiscord::new(cfg.token.clone()), cfg.policy()));
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let exit = runtime.block_on(dispatch(&cli, app, &mut out, &mut err));
    let _ = out.flush();
    exit.into()
}

/// Runs one command against `app`. Generic over the Discord port so tests can pass
/// `FakeDiscord`; `main_entry` passes the twilight adapter and real stdio.
pub async fn dispatch<P: DiscordPort + 'static>(
    cli: &Cli,
    app: Arc<App<P>>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let guild = cli.guild.as_deref();
    match &cli.command {
        Command::Mcp(_) => cli::mcp::run(app).await,
        Command::Guilds => cli::guilds::run(&app, cli.json, out, err).await,
        Command::Export(args) => cli::export::run(&app, guild, args, out, err).await,
        Command::Validate(args) => cli::validate::run(args, cli.json, out, err),
        Command::Schema(args) => cli::schema::run(args, out, err),
    }
}
