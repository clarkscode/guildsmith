//! `WebhooksPort`: create, list, edit, delete and post through webhooks (US5). Tokens only
//! travel as [`Secret`]s and never appear in [`WebhookInfo`].

use std::future::Future;

use guildsmith_core::Id;

use super::PortResult;
use super::messages::{MessageInfo, OutgoingMessage};
use crate::secret::Secret;
pub use guildsmith_core::live::{WebhookInfo, WebhookKind};

/// Webhook changes. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebhookFields {
    pub name: Option<String>,
    /// A data URI, or `Some(None)` to remove the avatar.
    #[allow(clippy::option_option)] // three states: leave alone, clear, set
    pub avatar: Option<Option<String>>,
    pub channel_id: Option<Id>,
}

pub trait WebhooksPort: Send + Sync {
    fn channel_webhooks(&self, channel: Id) -> impl Future<Output = PortResult<Vec<WebhookInfo>>> + Send;
    fn guild_webhooks(&self, guild: Id) -> impl Future<Output = PortResult<Vec<WebhookInfo>>> + Send;
    /// Creates an incoming webhook and returns it with its token.
    fn create_webhook(
        &self,
        channel: Id,
        name: &str,
        avatar: Option<&str>,
        reason: &str,
    ) -> impl Future<Output = PortResult<(WebhookInfo, Secret)>> + Send;
    fn update_webhook(
        &self,
        webhook: Id,
        fields: &WebhookFields,
        reason: &str,
    ) -> impl Future<Output = PortResult<WebhookInfo>> + Send;
    fn delete_webhook(&self, webhook: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// The token Discord returns for `GET webhooks/{id}`, if it returns one (research T063).
    fn webhook_token(&self, webhook: Id) -> impl Future<Output = PortResult<Option<Secret>>> + Send;
    /// Posts through a webhook and waits for the message.
    fn execute_webhook(
        &self,
        webhook: Id,
        token: &Secret,
        msg: &OutgoingMessage,
        thread: Option<Id>,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> impl Future<Output = PortResult<MessageInfo>> + Send;
}
