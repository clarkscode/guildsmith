//! Every group's tools, in a fixed order. `tools/list` shows the registered groups; the
//! dispatcher reaches the rest (MCP 2026-07-28: `tools/list` doesn't change during the process).

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::toolsets::Toolset;
use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::GuildsmithServer;

/// One group's tools. Each story adds its group here.
fn group<P: DiscordPort + 'static>(t: Toolset) -> ToolRouter<GuildsmithServer<P>> {
    match t {
        Toolset::Core => core::<P>(),
        Toolset::Moderation => GuildsmithServer::<P>::moderation_tools(),
        Toolset::Messages => GuildsmithServer::<P>::messages_group_tools(),
        Toolset::Threads => GuildsmithServer::<P>::threads_tools(),
        Toolset::Polls => GuildsmithServer::<P>::polls_tools(),
        Toolset::Events => GuildsmithServer::<P>::events_tools(),
        Toolset::Invites => GuildsmithServer::<P>::invites_tools(),
        Toolset::Voice => GuildsmithServer::<P>::voice_tools(),
        Toolset::Automod => GuildsmithServer::<P>::automod_tools(),
        Toolset::Webhooks => GuildsmithServer::<P>::webhooks_tools(),
        Toolset::Expressions => GuildsmithServer::<P>::expressions_tools(),
        Toolset::Settings => GuildsmithServer::<P>::settings_tools(),
        Toolset::Raw => GuildsmithServer::<P>::raw_tools(),
    }
}

/// Routes for every tool of every group.
#[must_use]
pub fn router<P: DiscordPort + 'static>() -> ToolRouter<GuildsmithServer<P>> {
    let mut router = ToolRouter::new();
    for t in Toolset::ALL {
        router += group::<P>(*t);
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
        + GuildsmithServer::<P>::dispatch_tools()
}
