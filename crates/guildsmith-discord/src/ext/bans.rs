//! Bulk ban (`POST guilds/{g}/bulk-ban`), missing in twilight 0.17.1.

use guildsmith_app::ports::{BulkBanResult, PortResult};
use guildsmith_core::Id;
use serde::{Deserialize, Serialize};
use twilight_http::Client;
use twilight_http::request::Method;

use crate::raw;

#[derive(Debug, Serialize)]
pub(crate) struct BulkBanBody {
    /// At most 200.
    pub user_ids: Vec<Id>,
    /// At most 604800.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_message_seconds: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BulkBanResponse {
    pub banned_users: Vec<Id>,
    pub failed_users: Vec<Id>,
}

/// Bans up to 200 users. Discord answers `500000` when none could be banned.
pub(crate) async fn bulk_ban(
    http: &Client,
    guild: Id,
    users: &[Id],
    delete_message_seconds: u32,
    reason: &str,
) -> PortResult<BulkBanResult> {
    let body = BulkBanBody {
        user_ids: users.to_vec(),
        delete_message_seconds: (delete_message_seconds > 0).then_some(delete_message_seconds),
    };
    let res: BulkBanResponse =
        raw::json(http, Method::Post, &format!("guilds/{guild}/bulk-ban"), &[], Some(&body), Some(reason)).await?;
    Ok(BulkBanResult { banned: res.banned_users, failed: res.failed_users })
}
