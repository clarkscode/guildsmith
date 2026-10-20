//! `RawPort`: any request to Discord's API through the same client, auth and rate limiter as
//! every other call (research E2/E3). Used by the `raw` group.

use std::future::Future;

use guildsmith_core::raw::RawRequest;
use serde_json::Value;

use super::PortResult;

/// A raw response. The adapter has already removed `token` fields and webhook URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReply {
    pub status: u16,
    /// `null` for an empty body.
    pub json: Value,
}

pub trait RawPort: Send + Sync {
    /// Sends `req`. `reason` becomes the audit-log reason (writes only). Reads are retried on
    /// 5xx and network errors; writes never are.
    fn raw_request(&self, req: &RawRequest, reason: Option<&str>) -> impl Future<Output = PortResult<RawReply>> + Send;
}
