//! guildsmith-app: use cases, ports and the safety policy shared by the CLI and MCP server.

pub mod app;
pub mod assets;
pub mod bulk;
pub mod cache;
pub mod config;
pub mod confirm;
pub mod date;
pub mod error;
pub mod policy;
pub mod ports;
pub mod preflight;
pub mod resolve;
pub mod secret;
pub mod secrets;
pub mod services;
pub mod toolsets;
pub mod writeback;

#[cfg(feature = "fake")]
pub mod fake;

pub use app::App;
