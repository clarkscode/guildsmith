//! `ExpressionsPort`: custom emoji, stickers and soundboard sounds (US6, research E4/E7).

use std::future::Future;

use guildsmith_core::Id;
use guildsmith_core::asset::FileType;

use super::PortResult;
pub use guildsmith_core::live::{ExpressionInfo, ExpressionKind};

/// An expression to upload. The service has checked the file.
#[derive(Debug, Clone, PartialEq)]
pub struct NewExpression {
    pub kind: ExpressionKind,
    pub name: String,
    pub bytes: Vec<u8>,
    pub file_type: FileType,
    pub roles: Vec<Id>,
    pub description: String,
    pub tags: String,
    pub volume: Option<f64>,
    pub emoji: Option<String>,
}

/// Expression changes. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExpressionEdit {
    pub name: Option<String>,
    pub roles: Option<Vec<Id>>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub volume: Option<f64>,
    /// `Some(None)` clears a sound's emoji.
    #[allow(clippy::option_option)] // three states: leave alone, clear, set
    pub emoji: Option<Option<String>>,
}

pub trait ExpressionsPort: Send + Sync {
    fn expressions(
        &self,
        guild: Id,
        kind: ExpressionKind,
    ) -> impl Future<Output = PortResult<Vec<ExpressionInfo>>> + Send;
    fn create_expression(
        &self,
        guild: Id,
        new: &NewExpression,
        reason: &str,
    ) -> impl Future<Output = PortResult<ExpressionInfo>> + Send;
    fn update_expression(
        &self,
        guild: Id,
        kind: ExpressionKind,
        id: Id,
        edit: &ExpressionEdit,
        reason: &str,
    ) -> impl Future<Output = PortResult<ExpressionInfo>> + Send;
    fn delete_expression(
        &self,
        guild: Id,
        kind: ExpressionKind,
        id: Id,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
}
