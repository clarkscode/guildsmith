//! `guildsmith doctor -g <guild>`: token, allowlist, permissions per toolset, role order
//! (FR-033). Exits 0 when every check passes, 3 otherwise.

use std::io::Write;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::doctor;

use super::Exit;
use crate::render;

pub async fn run<P: DiscordPort>(app: &App<P>, guild: Option<&str>, json: bool, out: &mut dyn Write) -> Exit {
    let report = doctor::doctor(app, guild).await;
    if json {
        render::json(out, &report);
    } else {
        render::doctor(out, &report);
    }
    if report.ok { Exit::Ok } else { Exit::Refused }
}
