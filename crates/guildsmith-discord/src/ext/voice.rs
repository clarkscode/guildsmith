//! Voice channel status (`PUT channels/{c}/voice-status`), missing in twilight 0.17.1.

use guildsmith_app::ports::PortResult;
use guildsmith_core::Id;
use serde_json::json;
use twilight_http::Client;
use twilight_http::request::Method;

use crate::raw;

/// Sets (≤ 500 characters) or clears (`None`) a voice channel's status.
pub(crate) async fn set_status(http: &Client, channel: Id, status: Option<&str>, reason: &str) -> PortResult<()> {
    let body = json!({"status": status.unwrap_or("")});
    raw::send(http, Method::Put, &format!("channels/{channel}/voice-status"), &[], Some(&body), Some(reason)).await?;
    Ok(())
}
