//! The confirmation guard for destructive live operations (FR-018, research R5).
//!
//! Without `confirm`, a destructive operation returns `{ preview: true, would: [...] }` and makes
//! no request. With it, the operation runs.

use std::future::Future;

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::ServiceError;

/// What a destructive operation would do. `preview` is always `true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Preview {
    pub preview: bool,
    pub would: Vec<String>,
}

/// Either a preview or the operation's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Outcome<T> {
    Preview(Preview),
    Done(T),
}

impl<T> Outcome<T> {
    #[must_use]
    pub fn is_preview(&self) -> bool {
        matches!(self, Outcome::Preview(_))
    }
}

/// Runs `op` only when `confirmed`; otherwise returns a preview of `would`.
pub async fn guarded<T, F, Fut>(confirmed: bool, would: Vec<String>, op: F) -> Result<Outcome<T>, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, ServiceError>>,
{
    if confirmed { op().await.map(Outcome::Done) } else { Ok(Outcome::Preview(Preview { preview: true, would })) }
}
