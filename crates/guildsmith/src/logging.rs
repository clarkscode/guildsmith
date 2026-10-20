//! Logging: `tracing` to **stderr only** (stdout carries the MCP protocol), with every line
//! scrubbed of the bot token, `Authorization` header values (FR-021), and webhook tokens in
//! `webhooks/<id>/<token>` paths and URLs (002 FR-030).

use std::io::{self, Write};
use std::sync::Arc;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

const REDACTED: &str = "<redacted>";

/// Removes secrets from text.
#[derive(Clone, Default)]
pub struct Scrubber {
    secrets: Arc<Vec<String>>,
}

impl Scrubber {
    #[must_use]
    pub fn new(secrets: Vec<String>) -> Self {
        // Very short strings would redact unrelated text; real tokens are long.
        let secrets = secrets.into_iter().filter(|s| s.len() >= 8).collect();
        Scrubber { secrets: Arc::new(secrets) }
    }

    /// Replaces every known secret, and any value after `authorization` + `Bot`/`Bearer`.
    #[must_use]
    pub fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for s in self.secrets.iter() {
            out = out.replace(s.as_str(), REDACTED);
        }
        scrub_webhooks(&scrub_authorization(&out))
    }
}

/// Replaces the token in every `webhooks/<id>/<token>`.
fn scrub_webhooks(text: &str) -> String {
    const MARK: &str = "webhooks/";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        let after = &rest[at + MARK.len()..];
        let digits = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
        let token_start = &after[digits..];
        if digits > 0
            && let Some(tail) = token_start.strip_prefix('/')
        {
            let len = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(tail.len());
            if len > 0 {
                out.push_str(&rest[..=at + MARK.len() + digits]);
                out.push_str(REDACTED);
                rest = &tail[len..];
                continue;
            }
        }
        out.push_str(&rest[..at + MARK.len()]);
        rest = after;
    }
    out.push_str(rest);
    out
}

fn scrub_authorization(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(found) = lower[i..].find("authorization") {
        let start = i + found + "authorization".len();
        out.push_str(&text[i..start]);
        // Skip separators like `": "`, `=`, quotes and spaces.
        let rest = &text[start..];
        let sep_len = rest.find(|c: char| !matches!(c, ':' | '=' | '"' | '\'' | ' ')).unwrap_or(rest.len());
        out.push_str(&rest[..sep_len]);
        let after = &rest[sep_len..];
        let scheme = ["Bot ", "Bearer ", "bot ", "bearer "].iter().find(|p| after.starts_with(**p));
        let mut j = start + sep_len;
        if let Some(p) = scheme {
            out.push_str(p);
            j += p.len();
        }
        let value_len = text[j..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | ')'))
            .unwrap_or(text.len() - j);
        if value_len > 0 {
            out.push_str(REDACTED);
        }
        i = j + value_len;
    }
    out.push_str(&text[i..]);
    out
}

/// A writer that scrubs each formatted log line before it reaches stderr.
pub struct ScrubWriter {
    scrubber: Scrubber,
}

impl Write for ScrubWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        io::stderr().lock().write_all(self.scrubber.scrub(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

/// Makes a `ScrubWriter` per log event.
#[derive(Clone)]
pub struct MakeScrubWriter {
    scrubber: Scrubber,
}

impl<'a> MakeWriter<'a> for MakeScrubWriter {
    type Writer = ScrubWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubWriter { scrubber: self.scrubber.clone() }
    }
}

/// Installs the global subscriber. `filter` is a `tracing` filter such as `warn` or
/// `guildsmith=debug`.
pub fn init(filter: &str, secrets: Vec<String>) {
    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(MakeScrubWriter { scrubber: Scrubber::new(secrets) })
        .with_ansi(false)
        .with_target(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secrets_are_redacted() {
        let s = Scrubber::new(vec!["gs-canary-7f3a-long".to_owned()]);
        assert_eq!(s.scrub("token=gs-canary-7f3a-long done"), "token=<redacted> done");
    }

    #[test]
    fn webhook_tokens_in_paths_and_urls_are_redacted() {
        let s = Scrubber::default();
        let line = r#"req{method="POST" url="https://discord.com/api/v10/webhooks/123/abc_DEF-xyz?wait=true"}"#;
        let out = s.scrub(line);
        assert!(!out.contains("abc_DEF-xyz"), "{out}");
        assert!(out.contains("webhooks/123/<redacted>?wait=true"), "{out}");
        assert_eq!(s.scrub("GET webhooks/123 done"), "GET webhooks/123 done", "no token, nothing to hide");
        assert_eq!(s.scrub("see webhooks/ and webhooks/x/y"), "see webhooks/ and webhooks/x/y");
    }

    #[test]
    fn authorization_values_are_redacted_even_when_unknown() {
        let s = Scrubber::default();
        let out = s.scrub(r#"headers: {"authorization": "Bot abc.def-ghi"} x"#);
        assert!(!out.contains("abc.def-ghi"), "{out}");
        assert!(out.contains("<redacted>"), "{out}");
        let out = s.scrub("Authorization: Bearer xyz123 next");
        assert_eq!(out, "Authorization: Bearer <redacted> next");
        assert_eq!(s.scrub("no secrets here"), "no secrets here");
    }
}
