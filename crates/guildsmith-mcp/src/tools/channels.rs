//! `channels_tree`.

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::inspect::{self, TreeNode};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::respond;
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TreeParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Extra fields: position, topic, nsfw.
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct TreeOutput {
    pub tree: Vec<TreeNode>,
}

#[tool_router(router = channel_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Show a server's categories and channels as a tree with names, kinds and ids.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<TreeOutput>()
    )]
    async fn channels_tree(&self, Parameters(p): Parameters<TreeParams>) -> CallToolResult {
        let res = async {
            let s = inspect::snapshot(&self.app, p.guild.as_deref()).await?;
            Ok(TreeOutput { tree: inspect::tree(&s, &p.fields)? })
        }
        .await;
        respond(res, |t| format!("{} top-level entries", t.tree.len()))
    }
}
