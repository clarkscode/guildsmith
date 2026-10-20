//! `AssetPort`: downloads for URL assets and Discord CDN files (research E7).

use std::future::Future;

use super::PortResult;

pub trait AssetPort: Send + Sync {
    /// GETs an `http(s)` URL and returns its body, refusing bodies over `max_bytes`.
    fn fetch_url(&self, url: &str, max_bytes: u64) -> impl Future<Output = PortResult<Vec<u8>>> + Send;
}
