//! The MCP server: stdio transport, enabled toolsets, cache hints on `tools/list`.

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CacheScope, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ResultType};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool_handler};

use crate::toolsets;

/// The tool list is fixed for the life of the process, so clients may cache it for an hour.
const TOOLS_TTL_MS: u64 = 60 * 60 * 1000;

/// guildsmith's MCP server.
pub struct GuildsmithServer<P: DiscordPort + 'static> {
    pub(crate) app: Arc<App<P>>,
    tool_router: ToolRouter<Self>,
}

impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    pub fn new(app: Arc<App<P>>) -> Self {
        let tool_router = toolsets::router(&app.policy.toolsets);
        GuildsmithServer { app, tool_router }
    }

    /// Names of the registered tools, in `tools/list` order.
    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect()
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "guildsmith",
    instructions = "Manage Discord servers: inspect structure, export it to guild.toml, plan and apply \
                    reviewed changes, and run day-to-day admin tools. Destructive tools return a preview \
                    unless confirm is true."
)]
impl<P: DiscordPort + 'static> ServerHandler for GuildsmithServer<P> {
    // Nothing to await: the tool list is fixed at startup.
    #[allow(clippy::unused_async_trait_impl)]
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let hints = context.protocol_version().is_some_and(|v| v >= ProtocolVersion::V_2026_07_28);
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
            ttl_ms: hints.then_some(TOOLS_TTL_MS),
            cache_scope: hints.then_some(CacheScope::Public),
        })
    }
}

/// Serves MCP on stdin/stdout until the client disconnects. stdout carries only protocol.
pub async fn serve_stdio<P: DiscordPort + 'static>(
    app: Arc<App<P>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = GuildsmithServer::new(app);
    tracing::info!(tools = server.tool_names().len(), "guildsmith mcp ready on stdio");
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}
