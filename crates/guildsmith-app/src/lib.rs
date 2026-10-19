//! guildsmith-app: use cases, ports and the safety policy shared by the CLI and MCP server.

pub mod app;
pub mod config;
pub mod date;
pub mod error;
pub mod policy;
pub mod ports;
pub mod resolve;
pub mod secret;
pub mod services;

#[cfg(feature = "fake")]
pub mod fake;

pub use app::App;
