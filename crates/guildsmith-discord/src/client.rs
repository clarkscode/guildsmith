//! The twilight HTTP client, built lazily on first use (research R17: no network at startup).
//!
//! Only bot tokens are supported (FR-022): a `Bearer ` token is refused before any request, and
//! the first use calls `GET users/@me` once and refuses accounts that aren't bots.

use std::sync::Arc;

use guildsmith_app::ports::{CurrentUser, PortError, PortResult};
use guildsmith_app::secret::Secret;
use tokio::sync::OnceCell;
use twilight_http::Client;

use crate::error::map_err;
use crate::ids;

/// A ready, verified client plus the bot it belongs to.
pub(crate) struct Ready {
    pub http: Arc<Client>,
    pub user: CurrentUser,
}

/// Holds the token until the first request, then a verified client.
pub(crate) struct LazyClient {
    token: Option<Secret>,
    /// `host:port` of an HTTP proxy (twilight's http-proxy, or wiremock in tests).
    proxy: Option<String>,
    ready: OnceCell<Ready>,
}

impl LazyClient {
    pub fn new(token: Option<Secret>, proxy: Option<String>) -> Self {
        LazyClient { token, proxy, ready: OnceCell::new() }
    }

    fn build(&self) -> PortResult<Client> {
        // rustls needs a process-wide crypto provider; installing twice is harmless.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let raw = self.token.as_ref().map(|t| t.expose().trim()).filter(|t| !t.is_empty()).ok_or(PortError::NoToken)?;
        if raw.starts_with("Bearer ") {
            return Err(PortError::NotBotToken);
        }
        let token = raw.strip_prefix("Bot ").unwrap_or(raw);
        let mut builder = Client::builder().token(format!("Bot {token}"));
        if let Some(proxy) = &self.proxy {
            // A proxy handles rate limits itself, so twilight's limiter is turned off.
            builder = builder.proxy(proxy.clone(), true).ratelimiter(None);
        }
        Ok(builder.build())
    }

    /// The verified client. The first call checks the token belongs to a bot.
    pub async fn get(&self) -> PortResult<&Ready> {
        self.ready
            .get_or_try_init(|| async {
                let http = self.build()?;
                let me = http.current_user().await.map_err(map_err)?.model().await.map_err(map_err)?;
                if !me.bot {
                    return Err(PortError::NotBotToken);
                }
                let user = CurrentUser { id: ids::from_user(me.id), username: me.name, bot: me.bot };
                Ok(Ready { http: Arc::new(http), user })
            })
            .await
    }
}
