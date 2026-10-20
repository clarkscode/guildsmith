//! Use cases. The CLI and the MCP server call these; neither contains business rules.

pub mod apply;
pub mod automod;
pub mod doctor;
pub mod events;
pub mod export;
pub mod expressions;
pub(crate) mod extras;
pub mod guilds;
pub mod inspect;
pub mod invites;
pub mod live;
pub mod messages;
pub mod moderation;
pub mod plan;
pub mod polls;
pub mod raw;
pub mod settings;
pub mod threads;
pub mod validate;
pub mod voice;
pub mod webhooks;
