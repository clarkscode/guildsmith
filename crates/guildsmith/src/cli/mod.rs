//! The command line (contracts/cli.md). Every command calls the same `guildsmith-app`
//! services as the matching MCP tool.

pub mod apply;
pub mod auth;
pub mod doctor;
pub mod export;
pub mod guilds;
pub mod mcp;
pub mod plan;
pub mod schema;
pub mod validate;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Fast MCP server and CLI for building and managing Discord servers.
// Doc comments here are clap help text, so env var names stay unquoted.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
#[command(name = "guildsmith", version, about, propagate_version = true)]
pub struct Cli {
    /// Config file. Precedence: env vars, then this file, then the default config file.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Target server: an id, or an exact server name the bot can see.
    #[arg(short = 'g', long, global = true, value_name = "ID|NAME")]
    pub guild: Option<String>,

    /// Refuse every write (same as GUILDSMITH_READ_ONLY=1).
    #[arg(long, global = true)]
    pub read_only: bool,

    /// Log filter, e.g. `info` or `guildsmith=debug` (same as GUILDSMITH_LOG). Logs go to stderr.
    #[arg(long, global = true, value_name = "FILTER")]
    pub log: Option<String>,

    /// Terminal colors for human output. NO_COLOR is honored.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Machine-readable output on stdout.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the MCP server on stdio.
    Mcp(mcp::McpArgs),
    /// List the servers the bot can access (filtered by the allowlist).
    Guilds,
    /// Export a server's structure as a spec file.
    Export(export::ExportArgs),
    /// Check a spec file offline.
    Validate(validate::ValidateArgs),
    /// Show what applying a spec file would change, with its plan hash.
    Plan(plan::PlanArgs),
    /// Apply a reviewed plan for a spec file.
    Apply(apply::ApplyArgs),
    /// Check the token, allowlist, the bot's permissions and role order.
    Doctor,
    /// Store, remove or check the bot token in the OS keyring.
    Auth(auth::AuthArgs),
    /// Print the JSON Schema for spec files.
    Schema(schema::SchemaArgs),
}

/// Exit codes (contracts/cli.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Success (for `plan --detailed-exitcode`: no changes).
    Ok = 0,
    /// Invalid input, network or Discord error, apply stopped partway.
    Error = 1,
    /// `plan --detailed-exitcode` only: the plan has changes.
    Changes = 2,
    /// Refused by policy or preflight.
    Refused = 3,
}

impl From<Exit> for std::process::ExitCode {
    fn from(e: Exit) -> Self {
        std::process::ExitCode::from(e as u8)
    }
}
