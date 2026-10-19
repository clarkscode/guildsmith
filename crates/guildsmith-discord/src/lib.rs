//! guildsmith-discord: the twilight-http adapter implementing `DiscordPort`.
//!
//! twilight types never leave this crate (constitution II). Write methods arrive with the
//! plan/apply and live-tool stories; until then they return `PortError::Unsupported`.

mod client;
mod error;
mod ids;
mod map;
mod port;
mod retry;
mod snapshot;

pub use port::TwilightDiscord;
