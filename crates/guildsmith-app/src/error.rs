//! Errors from application services. Both interfaces map them the same way: refusals to exit
//! code 3 / MCP `refusal`, everything else to exit code 1 / a tool error.

use guildsmith_core::Id;

use crate::policy::{Refusal, RefusalKind};
use crate::ports::PortError;
use crate::resolve::Candidate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    /// Refused by policy: nothing was changed.
    #[error("{0}")]
    Refused(Refusal),
    #[error("{0}")]
    Port(PortError),
    #[error("no {what} matches `{input}`{}", hint_suggestions(.suggestions))]
    NotFound { what: &'static str, input: String, suggestions: Vec<String> },
    #[error("`{input}` matches more than one {what}: {}", list_candidates(.candidates))]
    Ambiguous { what: &'static str, input: String, candidates: Vec<Candidate> },
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(String),
}

fn hint_suggestions(s: &[String]) -> String {
    if s.is_empty() { String::new() } else { format!("; did you mean {}?", s.join(", ")) }
}

fn list_candidates(c: &[Candidate]) -> String {
    c.iter()
        .map(|c| match &c.parent {
            Some(p) => format!("{} in {p} ({})", c.name, c.id),
            None => format!("{} ({})", c.name, c.id),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl From<PortError> for ServiceError {
    /// A token that isn't a bot's is refused like a policy rule (FR-022, exit code 3).
    fn from(e: PortError) -> Self {
        match e {
            PortError::NotBotToken => ServiceError::Refused(Refusal::new(
                RefusalKind::NotBotToken,
                "not a bot token: this token does not belong to a bot application; nothing was done",
                Some("create a bot in the Discord developer portal and use its token (only bot tokens are supported)"),
            )),
            other => ServiceError::Port(other),
        }
    }
}

impl From<Refusal> for ServiceError {
    fn from(r: Refusal) -> Self {
        ServiceError::Refused(r)
    }
}

impl ServiceError {
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        matches!(self, ServiceError::Refused(_))
    }

    /// The server a `NotInGuild` error is about, if any.
    #[must_use]
    pub fn not_in_guild(&self) -> Option<Id> {
        match self {
            ServiceError::Port(PortError::NotInGuild(id)) => Some(*id),
            _ => None,
        }
    }
}
