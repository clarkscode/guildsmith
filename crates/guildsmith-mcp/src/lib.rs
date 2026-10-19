//! guildsmith-mcp: the MCP server. Tools only translate input and output; every rule lives in
//! guildsmith-app (constitution II).

pub mod server;
pub mod shape;
mod tools;
pub mod toolsets;

pub use server::{GuildsmithServer, serve_stdio};
