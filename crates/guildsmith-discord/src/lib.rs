//! guildsmith-discord: the twilight-http adapter implementing `DiscordPort`.
//!
//! twilight types never leave this crate (constitution II).

mod client;
mod error;
mod ids;
mod map;
mod port;
mod retry;
mod snapshot;
mod write;

pub use port::TwilightDiscord;
