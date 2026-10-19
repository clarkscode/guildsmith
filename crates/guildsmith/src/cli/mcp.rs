//! `guildsmith mcp`: the MCP server on stdio.

use std::sync::Arc;

use clap::Args;
use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;

use super::Exit;

// Doc comments here are clap help text.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Args)]
pub struct McpArgs {
    /// Toolsets to register, e.g. `core,moderation` (overrides GUILDSMITH_TOOLSETS).
    #[arg(long, value_name = "LIST")]
    pub toolsets: Option<String>,
}

pub async fn run<P: DiscordPort + 'static>(app: Arc<App<P>>) -> Exit {
    match guildsmith_mcp::serve_stdio(app).await {
        Ok(()) => Exit::Ok,
        Err(e) => {
            tracing::error!(error = %e, "mcp server stopped");
            Exit::Error
        }
    }
}
