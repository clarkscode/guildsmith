//! Which tools are registered: only the toolsets enabled at startup, in a fixed order
//! (MCP 2026-07-28: `tools/list` doesn't change during the process).

use std::collections::BTreeSet;

use guildsmith_app::policy::Toolset;
use guildsmith_app::ports::DiscordPort;
use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::GuildsmithServer;

/// The router for the enabled toolsets. Each story adds its tools to the matching toolset.
#[must_use]
pub fn router<P: DiscordPort + 'static>(enabled: &BTreeSet<Toolset>) -> ToolRouter<GuildsmithServer<P>> {
    let mut router = ToolRouter::new();
    for toolset in enabled {
        router += match toolset {
            Toolset::Core => core::<P>(),
            Toolset::Moderation => moderation::<P>(),
        };
    }
    router
}

fn core<P: DiscordPort + 'static>() -> ToolRouter<GuildsmithServer<P>> {
    GuildsmithServer::<P>::guild_tools()
        + GuildsmithServer::<P>::spec_tools()
        + GuildsmithServer::<P>::role_tools()
        + GuildsmithServer::<P>::channel_tools()
        + GuildsmithServer::<P>::perm_tools()
        + GuildsmithServer::<P>::member_tools()
        + GuildsmithServer::<P>::message_tools()
}

fn moderation<P: DiscordPort + 'static>() -> ToolRouter<GuildsmithServer<P>> {
    GuildsmithServer::<P>::moderation_tools()
}
