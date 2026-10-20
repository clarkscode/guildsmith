//! The dispatcher (research E1, ADR-004): `toolsets` lists, describes and unlocks groups, and
//! `tool_call` runs a tool in an unlocked group through the same handler as a direct call, so
//! the agent can use a group mid-session without the tool list changing.

use guildsmith_app::policy::{Refusal, RefusalKind};
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::toolsets::Toolset;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, JsonObject};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::server::GuildsmithServer;
use crate::shape;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsetsAction {
    List,
    Describe,
    Enable,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ToolsetsParams {
    /// list: every group; describe: one group's tool definitions; enable: unlock a group.
    pub action: ToolsetsAction,
    /// The group for describe and enable, e.g. polls.
    #[serde(default)]
    pub toolset: Option<String>,
}

/// One group in `toolsets list`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GroupRow {
    pub name: String,
    pub purpose: String,
    pub tools: usize,
    /// Approximate tokens the group's tool definitions cost an agent (bytes ÷ 3.5).
    pub approx_tokens: usize,
    pub required_permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_intents: Vec<String>,
    /// In `tools/list` (chosen by the operator at startup).
    pub registered: bool,
    /// Callable: registered, or enabled during this session.
    pub unlocked: bool,
    /// Blocked by the operator; can't be enabled.
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ToolsetsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolsets: Option<Vec<GroupRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolset: Option<String>,
    /// For describe: each tool's name, description, inputSchema and annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlocked: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ToolCallParams {
    /// The tool to run, e.g. poll_create. Its group must be unlocked (see toolsets).
    pub tool: String,
    /// The tool's arguments, as its inputSchema describes.
    #[serde(default)]
    pub arguments: Option<JsonObject>,
}

fn approx(bytes: usize) -> usize {
    bytes * 2 / 7
}

fn parse_group(name: Option<&str>) -> Result<Toolset, CallToolResult> {
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Err(shape::error("`toolset` is needed for describe and enable", &json!({"kind": "invalid"})));
    };
    name.parse::<Toolset>().map_err(|e| shape::error(&e.to_string(), &json!({"kind": "invalid"})))
}

impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    /// Every group as `toolsets list` reports it (also used by `guildsmith toolsets`).
    #[must_use]
    pub fn toolset_rows(&self) -> Vec<GroupRow> {
        Toolset::ALL.iter().map(|t| self.group_row(*t)).collect()
    }

    fn group_row(&self, t: Toolset) -> GroupRow {
        let defs = self.group_tools(t);
        let bytes: usize = defs.iter().map(|d| serde_json::to_vec(d).map_or(0, |v| v.len())).sum();
        let state = &self.app.toolsets;
        GroupRow {
            name: t.name().to_owned(),
            purpose: t.purpose().to_owned(),
            tools: t.tools().len(),
            approx_tokens: approx(bytes),
            required_permissions: t.required_permissions().names(),
            required_intents: t.required_intents().iter().map(|s| (*s).to_owned()).collect(),
            registered: state.registered().contains(&t),
            unlocked: state.is_unlocked(t),
            blocked: state.blocked().contains(&t),
        }
    }
}

#[tool_router(router = dispatch_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "List tool groups, describe one group's tools, or enable a group for this session.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<ToolsetsOutput>()
    )]
    async fn toolsets(&self, Parameters(p): Parameters<ToolsetsParams>) -> CallToolResult {
        match p.action {
            ToolsetsAction::List => {
                let rows = self.toolset_rows();
                let open = rows.iter().filter(|r| r.unlocked).count();
                let summary = format!("{} groups, {open} unlocked; enable one, then use tool_call", rows.len());
                shape::ok(&summary, &ToolsetsOutput { toolsets: Some(rows), ..ToolsetsOutput::default() })
            }
            ToolsetsAction::Describe => {
                let t = match parse_group(p.toolset.as_deref()) {
                    Ok(t) => t,
                    Err(e) => return e,
                };
                let tools: Vec<Value> = self
                    .group_tools(t)
                    .into_iter()
                    .map(|d| {
                        json!({
                            "name": d.name,
                            "description": d.description,
                            "inputSchema": Value::Object(d.input_schema.as_ref().clone()),
                            "annotations": d.annotations,
                        })
                    })
                    .collect();
                let summary = format!("{} tool(s) in {t}", tools.len());
                let out = ToolsetsOutput {
                    toolset: Some(t.name().to_owned()),
                    tools: Some(tools),
                    ..ToolsetsOutput::default()
                };
                shape::ok(&summary, &out)
            }
            ToolsetsAction::Enable => {
                let t = match parse_group(p.toolset.as_deref()) {
                    Ok(t) => t,
                    Err(e) => return e,
                };
                match self.app.toolsets.enable(t, self.app.clock.as_ref()) {
                    Ok(_) => {
                        let summary =
                            format!("{t} is unlocked; call its tools with tool_call (toolsets describe lists them)");
                        let out = ToolsetsOutput {
                            toolset: Some(t.name().to_owned()),
                            unlocked: Some(true),
                            ..ToolsetsOutput::default()
                        };
                        shape::ok(&summary, &out)
                    }
                    Err(r) => shape::refusal(&r),
                }
            }
        }
    }

    #[tool(
        description = "Run a tool from an unlocked group and return its result; the tool's own rules apply.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true)
    )]
    async fn tool_call(
        &self,
        Parameters(p): Parameters<ToolCallParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let name = p.tool.trim().to_owned();
        if matches!(name.as_str(), "toolsets" | "tool_call") {
            return shape::error(&format!("call {name} directly, not through tool_call"), &json!({"kind": "invalid"}));
        }
        let Some(group) = Toolset::of_tool(&name) else {
            return shape::error(
                &format!("no tool named `{name}`; toolsets describe lists each group's tools"),
                &json!({
                    "kind": "not_found"
                }),
            );
        };
        if !self.app.toolsets.is_unlocked(group) {
            let r = if self.app.toolsets.blocked().contains(&group) {
                Refusal::new(
                    RefusalKind::Blocked,
                    format!("{name} is in the {group} toolset, which the operator blocked"),
                    Some("the operator can unblock it in GUILDSMITH_TOOLSETS_BLOCK; agents can't"),
                )
            } else {
                Refusal::new(
                    RefusalKind::Locked,
                    format!("{name} is in the {group} toolset, which isn't unlocked"),
                    Some(&format!("call toolsets with action enable and toolset {group} first")),
                )
            };
            return shape::refusal(&r);
        }
        let mut request = CallToolRequestParams::new(name);
        request.arguments = p.arguments;
        match self.tool_router.call(ToolCallContext::new(self, request, ctx)).await {
            Ok(CallToolResponse::Complete(result)) => result,
            Ok(_) => {
                shape::error("the tool asked for input that tool_call can't relay", &json!({"kind": "unsupported"}))
            }
            Err(e) => shape::error(&e.message, &json!({"kind": "invalid"})),
        }
    }
}
