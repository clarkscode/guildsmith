//! guildsmith-mcp: the MCP server. Tools only translate input and output; every rule lives in
//! guildsmith-app (constitution II).

pub mod server;
pub mod shape;
// Tool parameter docs become schema descriptions for agents: plain words, no backticks.
#[allow(clippy::doc_markdown)]
mod tools;
pub mod toolsets;

pub use server::{GuildsmithServer, serve_stdio};
