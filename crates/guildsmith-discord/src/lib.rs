//! guildsmith-discord: the twilight-http adapter implementing `DiscordPort`.
//!
//! twilight types never leave this crate (constitution II).

mod automod;
mod cdn;
mod client;
mod error;
mod events;
mod expressions;
mod ext;
mod ids;
mod invites;
mod map;
mod messages;
mod moderation;
mod polls;
mod port;
mod raw;
mod retry;
mod settings;
mod snapshot;
mod threads;
mod voice;
mod webhooks;
mod write;

pub use port::TwilightDiscord;
pub use raw::{is_webhook_url, redact};
