//! `guildsmith doctor -g <guild>`: token, allowlist, permissions per toolset, role order
//! (FR-033). Exits 0 when every check passes, 3 otherwise.

use std::io::Write;

use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::doctor::{self, Check, Status};
use guildsmith_mcp::GuildsmithServer;

use super::Exit;
use crate::render;

pub async fn run<P: DiscordPort + 'static>(
    app: &Arc<App<P>>,
    guild: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Exit {
    let mut report = doctor::doctor(app, guild).await;
    // What the MCP server would register: tool count and context cost (FR-010).
    let (tools, tokens) = GuildsmithServer::new(app.clone()).listed_cost();
    let groups: Vec<&str> = app.toolsets.registered().iter().map(|t| t.name()).collect();
    report.checks.push(Check {
        name: "tools".to_owned(),
        status: Status::Pass,
        detail: format!("{tools} tools from {} (≈{tokens} tokens of tool definitions)", groups.join(", ")),
        problems: Vec::new(),
    });
    if json {
        render::json(out, &report);
    } else {
        render::doctor(out, &report);
    }
    if report.ok { Exit::Ok } else { Exit::Refused }
}
