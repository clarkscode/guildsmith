//! The plan hash (research R4): stateless, so the CLI and MCP server check it the same way and
//! it survives restarts.
//!
//! `gsp_` + the first 32 hex characters (128 bits) of
//! `blake3(canonical_json(ops) ‖ snapshot_fingerprint ‖ spec_digest ‖ engine_version)`.
//! Any change to the server changes its fingerprint and so the hash.

use crate::planner::Op;
use crate::spec::GuildSpec;

/// Changes whenever the planner's behavior could change for the same input.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PREFIX: &str = "gsp_";

/// blake3 of the parsed spec in canonical JSON form, so formatting and comments don't matter.
#[must_use]
pub fn spec_digest(spec: &GuildSpec) -> String {
    let canonical = serde_json::to_vec(spec).expect("spec serializes");
    blake3::hash(&canonical).to_hex().to_string()
}

/// The plan hash for `ops` planned from `spec` against a snapshot with `fingerprint`.
#[must_use]
pub fn plan_hash(ops: &[Op], fingerprint: &str, spec: &GuildSpec) -> String {
    let mut h = blake3::Hasher::new();
    // A separator between parts, so one part can't run into the next.
    for part in [
        serde_json::to_vec(ops).expect("ops serialize").as_slice(),
        fingerprint.as_bytes(),
        spec_digest(spec).as_bytes(),
        ENGINE_VERSION.as_bytes(),
    ] {
        h.update(&(part.len() as u64).to_le_bytes());
        h.update(part);
    }
    let hex = h.finalize().to_hex();
    format!("{PREFIX}{}", &hex[..32])
}

/// Whether `s` looks like a plan hash (`gsp_` + 32 lowercase hex characters).
#[must_use]
pub fn is_plan_hash(s: &str) -> bool {
    s.strip_prefix(PREFIX)
        .is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
