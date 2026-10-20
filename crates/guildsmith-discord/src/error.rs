//! Maps twilight errors to `PortError`. Error text never includes the token.

use guildsmith_app::ports::PortError;
use twilight_http::api_error::ApiError;
use twilight_http::error::ErrorType;

/// Anything twilight can fail with: request errors and body parse errors.
pub(crate) trait TwilightError {
    fn into_port(self) -> PortError;
}

impl TwilightError for twilight_http::Error {
    fn into_port(self) -> PortError {
        match self.kind() {
            ErrorType::Response { error, status, .. } => {
                let status = status.get();
                let (code, message) = match error {
                    ApiError::General(g) => (Some(g.code), g.message.clone()),
                    ApiError::Ratelimited(r) => (None, format!("rate limited; retry after {}s", r.retry_after)),
                    other => (None, other.to_string()),
                };
                match status {
                    401 => PortError::Unauthorized,
                    403 => PortError::Forbidden(message),
                    404 => PortError::NotFound(message),
                    _ => PortError::Discord { status, code, message },
                }
            }
            ErrorType::Unauthorized => PortError::Unauthorized,
            ErrorType::RequestError | ErrorType::RequestTimedOut | ErrorType::RequestCanceled => {
                PortError::Network(self.to_string())
            }
            ErrorType::Validation => PortError::Discord { status: 400, code: None, message: self.to_string() },
            // An error response whose body isn't Discord's JSON: twilight drops the status. In
            // practice that's a gateway or proxy page in front of Discord (502/503/504), so it
            // counts as a network error and reads retry it.
            ErrorType::Parsing { body } => {
                PortError::Network(format!("Discord returned an error with an unreadable body ({} bytes)", body.len()))
            }
            _ => PortError::Discord { status: 0, code: None, message: format!("unexpected response: {self}") },
        }
    }
}

impl TwilightError for twilight_http::response::DeserializeBodyError {
    fn into_port(self) -> PortError {
        PortError::Discord { status: 0, code: None, message: format!("couldn't read Discord's response: {self}") }
    }
}

pub(crate) fn map_err<E: TwilightError>(e: E) -> PortError {
    e.into_port()
}
