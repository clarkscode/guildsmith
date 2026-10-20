//! `AssetPort` for the fake: URLs answer with bytes set by tests; anything else is a 404.

use super::FakeDiscord;
use crate::ports::{AssetPort, PortError, PortResult};

impl FakeDiscord {
    /// Serves `bytes` at `url` (a URL asset, or a CDN file).
    pub fn set_url(&self, url: &str, bytes: Vec<u8>) {
        self.lock().urls.insert(url.to_owned(), bytes);
    }
}

/// An uploaded image (a data URI) as the CDN would hold it: its hash and bytes.
pub(crate) fn uploaded(data_uri: &str) -> Option<(String, Vec<u8>)> {
    let bytes = guildsmith_core::asset::decode_data(data_uri).ok()?;
    let hash = guildsmith_core::asset::hash(&bytes)[..32].to_owned();
    Some((hash, bytes))
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl AssetPort for FakeDiscord {
    async fn fetch_url(&self, url: &str, max_bytes: u64) -> PortResult<Vec<u8>> {
        self.read("fetch_url", |st| {
            let bytes = st.urls.get(url).cloned().ok_or_else(|| PortError::NotFound(format!("url {url}")))?;
            if bytes.len() as u64 > max_bytes {
                return Err(PortError::Discord {
                    status: 0,
                    code: None,
                    message: format!("{url} is over {max_bytes} bytes"),
                });
            }
            Ok(bytes)
        })
    }
}
