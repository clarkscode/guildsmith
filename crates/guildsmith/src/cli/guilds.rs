//! `guildsmith guilds`: servers the bot can access, filtered by the allowlist.

use std::io::Write;

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::guilds;

use super::Exit;
use crate::render;

pub async fn run<P: DiscordPort>(app: &App<P>, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    match guilds::list(app, false).await {
        Ok(list) => {
            if json {
                render::json(out, &list);
            } else {
                render::guilds(out, &list);
            }
            Exit::Ok
        }
        Err(e) => render::service_error(err, &e),
    }
}
