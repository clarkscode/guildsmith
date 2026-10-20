//! The MCP server: stdio transport, registered toolsets, cache hints on `tools/list`, and the
//! dispatcher's view of every group (research E1).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::toolsets::Toolset;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ResultType, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool_handler};
use serde_json::Value;

use crate::toolsets;

/// The tool list is fixed for the life of the process, so clients may cache it for an hour.
const TOOLS_TTL_MS: u64 = 60 * 60 * 1000;

/// guildsmith's MCP server.
pub struct GuildsmithServer<P: DiscordPort + 'static> {
    pub(crate) app: Arc<App<P>>,
    /// Routes for every tool of every group. Direct calls reach only listed tools; the
    /// dispatcher reaches any unlocked group.
    pub(crate) tool_router: ToolRouter<Self>,
    /// `tools/list`, built once: the registered groups, fixed for the life of the process.
    tools: Vec<Tool>,
    /// Every tool's compact definition, by name (for `toolsets describe` and budgets).
    pub(crate) catalog: BTreeMap<String, Tool>,
    /// Set by the CLI on Ctrl-C: bulk tools stop between items and still report what they did.
    stop: Option<Arc<AtomicBool>>,
}

/// Removes what agents don't need from a schema: descriptions and numeric formats. Used on
/// output schemas, whose results describe themselves (research R16 token budget). Keys of a
/// `properties` map are property names, not keywords, so they are kept.
fn compact(v: &mut Value, drop_descriptions: bool) {
    match v {
        Value::Object(map) => {
            map.remove("format");
            if drop_descriptions {
                map.remove("description");
            }
            for (key, child) in map.iter_mut() {
                if key == "properties" || key == "$defs" || key == "definitions" || key == "patternProperties" {
                    if let Value::Object(named) = child {
                        named.values_mut().for_each(|c| compact(c, drop_descriptions));
                    }
                } else {
                    compact(child, drop_descriptions);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|c| compact(c, drop_descriptions)),
        _ => {}
    }
}

fn compact_tool(mut tool: Tool) -> Tool {
    let mut input = Value::Object(tool.input_schema.as_ref().clone());
    compact(&mut input, false);
    if let Value::Object(map) = input {
        tool.input_schema = Arc::new(map);
    }
    if let Some(out) = &tool.output_schema {
        let mut out = Value::Object(out.as_ref().clone());
        compact(&mut out, true);
        if let Value::Object(map) = out {
            tool.output_schema = Some(Arc::new(map));
        }
    }
    tool
}

impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    pub fn new(app: Arc<App<P>>) -> Self {
        let tool_router = toolsets::router::<P>();
        let catalog: BTreeMap<String, Tool> =
            tool_router.list_all().into_iter().map(|t| (t.name.to_string(), compact_tool(t))).collect();
        let tools = app
            .toolsets
            .registered()
            .iter()
            .flat_map(|t| t.tools())
            .filter_map(|name| catalog.get(*name).cloned())
            .collect();
        GuildsmithServer { app, tool_router, tools, catalog, stop: None }
    }

    /// The server behind `guildsmith <group> <tool>`: every tool is callable directly (the
    /// operator, not an agent, chose it), with the same handlers and policy. `stop` is the
    /// Ctrl-C flag.
    pub fn for_cli(app: Arc<App<P>>, stop: Arc<AtomicBool>) -> Self {
        let mut server = Self::new(app);
        server.tools =
            Toolset::ALL.iter().flat_map(|t| t.tools()).filter_map(|name| server.catalog.get(*name).cloned()).collect();
        server.stop = Some(stop);
        server
    }

    /// Any tool's definition, listed or not.
    #[must_use]
    pub fn tool_definition(&self, name: &str) -> Option<Tool> {
        self.catalog.get(name).cloned()
    }

    /// The CLI's Ctrl-C flag, if this server serves the CLI.
    pub(crate) fn stop_flag(&self) -> Option<Arc<AtomicBool>> {
        self.stop.clone()
    }

    /// Names of the registered tools, in `tools/list` order.
    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name.to_string()).collect()
    }

    /// Whether `name` is in `tools/list`.
    #[must_use]
    pub fn is_listed(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name)
    }

    /// A group's tool definitions, in order (only tools that are implemented).
    #[must_use]
    pub fn group_tools(&self, t: Toolset) -> Vec<Tool> {
        t.tools().iter().filter_map(|n| self.catalog.get(*n).cloned()).collect()
    }

    /// The listed tools' count and approximate tokens of their full definitions (FR-010).
    #[must_use]
    pub fn listed_cost(&self) -> (usize, usize) {
        let bytes: usize = self.tools.iter().map(|t| serde_json::to_vec(t).map_or(0, |v| v.len())).sum();
        (self.tools.len(), bytes * 2 / 7)
    }

    /// The approximate description tokens of a set of tools (UTF-8 bytes ÷ 3.5, research R16).
    #[must_use]
    pub fn approx_tokens(tools: &[Tool]) -> usize {
        let bytes: usize = tools.iter().map(|t| t.description.as_deref().map_or(0, str::len)).sum();
        bytes * 2 / 7
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
    /// Direct calls reach only the tools in `tools/list`; other groups go through `tool_call`.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.is_listed(&request.name) {
            let hint = match Toolset::of_tool(&request.name) {
                Some(t) => format!(
                    "tool not found: {} is in the {t} toolset, which isn't registered; enable it with \
                     toolsets and call it through tool_call",
                    request.name
                ),
                None => format!("tool not found: {}", request.name),
            };
            return Err(ErrorData::invalid_params(hint, None));
        }
        self.tool_router.call(ToolCallContext::new(self, request, context)).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

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
            tools: self.tools.clone(),
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
    let (tools, tokens) = server.listed_cost();
    let groups: Vec<&str> = server.app.toolsets.registered().iter().map(|t| t.name()).collect();
    tracing::info!(tools, approx_tokens = tokens, toolsets = %groups.join(","), "guildsmith mcp ready on stdio");
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::compact;

    #[test]
    fn compaction_keeps_properties_named_like_keywords() {
        let mut schema = json!({
            "type": "object",
            "description": "drop me",
            "properties": {
                "description": {"type": "string", "description": "drop me too"},
                "format": {"type": "integer", "format": "uint32"},
            },
            "additionalProperties": false,
        });
        compact(&mut schema, true);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {"description": {"type": "string"}, "format": {"type": "integer"}},
                "additionalProperties": false,
            })
        );
    }
}
