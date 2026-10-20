//! Plain HTTP(S) downloads: URL assets and Discord CDN files for plan comparison (research
//! E7). Uses the same hyper/rustls stack as twilight, without the bot token: these requests
//! never carry `Authorization`.

use std::time::Duration;

use guildsmith_app::ports::{AssetPort, PortError, PortResult};
use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::port::TwilightDiscord;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 3;

fn bad(url: &str, why: impl std::fmt::Display) -> PortError {
    PortError::Discord { status: 0, code: None, message: format!("can't download {url}: {why}") }
}

/// GETs `url`, following up to 3 redirects, with a body limit and a timeout.
pub(crate) async fn fetch(url: &str, max_bytes: u64) -> PortResult<Vec<u8>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let https = hyper_rustls::HttpsConnectorBuilder::new().with_webpki_roots().https_or_http().enable_http1().build();
    let client: Client<_, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
    let mut current = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let uri: hyper::Uri = current.parse().map_err(|e| bad(url, e))?;
        if !matches!(uri.scheme_str(), Some("http" | "https")) {
            return Err(bad(url, "only http and https URLs can be fetched"));
        }
        let req = hyper::Request::get(uri)
            .header(hyper::header::USER_AGENT, "guildsmith (https://github.com/clarkscode/guildsmith)")
            .body(Empty::new())
            .map_err(|e| bad(url, e))?;
        let res = tokio::time::timeout(TIMEOUT, client.request(req))
            .await
            .map_err(|_| PortError::Network(format!("timed out downloading {url}")))?
            .map_err(|e| PortError::Network(format!("downloading {url}: {e}")))?;
        let status = res.status();
        if status.is_redirection() {
            let next = res.headers().get(hyper::header::LOCATION).and_then(|l| l.to_str().ok());
            current = next.ok_or_else(|| bad(url, "redirect without a location"))?.to_owned();
            continue;
        }
        if !status.is_success() {
            return Err(PortError::Discord {
                status: status.as_u16(),
                code: None,
                message: format!("downloading {url}: HTTP {status}"),
            });
        }
        let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let body = tokio::time::timeout(TIMEOUT, Limited::new(res.into_body(), limit).collect())
            .await
            .map_err(|_| PortError::Network(format!("timed out downloading {url}")))?
            .map_err(|e| bad(url, format!("{e} (limit {max_bytes} bytes)")))?;
        return Ok(body.to_bytes().to_vec());
    }
    Err(bad(url, "too many redirects"))
}

impl AssetPort for TwilightDiscord {
    async fn fetch_url(&self, url: &str, max_bytes: u64) -> PortResult<Vec<u8>> {
        fetch(url, max_bytes).await
    }
}
