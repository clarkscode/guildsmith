//! `guildsmith toolsets`: every tool group with its tool count, approximate tokens, the
//! permissions it needs, and whether it is registered or blocked (FR-010, contracts/cli.md).

use std::io::Write;
use std::sync::Arc;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_mcp::GuildsmithServer;

use super::Exit;
use crate::render;

pub fn run<P: DiscordPort + 'static>(app: &Arc<App<P>>, json: bool, out: &mut dyn Write) -> Exit {
    let server = GuildsmithServer::new(app.clone());
    let rows = server.toolset_rows();
    if json {
        render::json(out, &rows);
        return Exit::Ok;
    }
    for r in &rows {
        let state = if r.blocked {
            "blocked"
        } else if r.registered {
            "registered"
        } else {
            "available"
        };
        let _ = writeln!(out, "{:<12} {:>2} tools  ≈{:>5} tokens  {state}", r.name, r.tools, r.approx_tokens);
        let _ = writeln!(out, "    {}", r.purpose);
        if !r.required_permissions.is_empty() {
            let _ = writeln!(out, "    needs: {}", r.required_permissions.join(", "));
        }
        if !r.required_intents.is_empty() {
            let _ = writeln!(out, "    intents: {}", r.required_intents.join(", "));
        }
    }
    let (tools, tokens) = server.listed_cost();
    let _ = writeln!(out, "Registered now: {tools} tools (≈{tokens} tokens). Enable more with GUILDSMITH_TOOLSETS.");
    Exit::Ok
}
