//! `guilds_list` and `guild_overview`.

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::guilds::{self, GuildList};
use guildsmith_app::services::inspect::{self, Overview};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool;
use rmcp::tool_router;

use super::{GuildParam, respond};
use crate::server::GuildsmithServer;

#[tool_router(router = guild_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "List the servers the bot can access, filtered by the allowlist.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<GuildList>()
    )]
    async fn guilds_list(&self) -> CallToolResult {
        let res = guilds::list(&self.app, false).await;
        respond(res, |l| format!("{} server(s) available", l.guilds.len()))
    }

    #[tool(
        description = "Summarize a server: settings, counts, the role list and the channel tree.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Overview>()
    )]
    async fn guild_overview(&self, Parameters(p): Parameters<GuildParam>) -> CallToolResult {
        let res = inspect::overview(&self.app, p.guild.as_deref()).await;
        respond(res, |o| {
            format!(
                "{}: {} roles, {} categories, {} channels",
                o.guild.name, o.counts.roles, o.counts.categories, o.counts.channels
            )
        })
    }
}
