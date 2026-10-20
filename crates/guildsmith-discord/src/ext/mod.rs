//! Endpoints twilight 0.17.1 lacks (research E3), each a small serde request/response sent
//! through [`crate::raw`], so they share the client's auth, rate limiter and audit header.
//!
//! Delete a module once a twilight release covers it, keeping a test that the payload is the
//! same.

pub(crate) mod automod;
pub(crate) mod bans;
pub(crate) mod events;
pub(crate) mod guild;
pub(crate) mod invites;
pub(crate) mod moderation;
pub(crate) mod search;
pub(crate) mod soundboard;
pub(crate) mod voice;
