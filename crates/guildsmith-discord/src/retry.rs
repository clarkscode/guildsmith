//! Retries for reads only: 5xx and network errors, with jittered backoff. Mutations are never
//! retried (FR-013); 429s are handled by twilight's rate limiter.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use guildsmith_app::ports::PortResult;

const ATTEMPTS: u32 = 4;
const BASE_MS: u64 = 100;

fn jitter_ms() -> u64 {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    u64::from(nanos % 100)
}

/// Runs a read, retrying retryable errors up to `ATTEMPTS` times.
pub(crate) async fn read<T, F, Fut>(mut op: F) -> PortResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = PortResult<T>>,
{
    let mut attempt = 0;
    loop {
        match op().await {
            Err(e) if e.is_retryable() && attempt + 1 < ATTEMPTS => {
                let delay = BASE_MS * 2u64.pow(attempt) + jitter_ms();
                tracing::debug!(attempt, delay_ms = delay, error = %e, "retrying read");
                tokio::time::sleep(Duration::from_millis(delay)).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}
