//! `spec_validate` / `guildsmith validate`: offline checks (FR-006).

use guildsmith_core::validate::{Problem, validate_text};
use serde::Serialize;

/// The result of validating spec text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ValidateOutput {
    pub valid: bool,
    pub problems: Vec<Problem>,
}

#[must_use]
pub fn validate(text: &str) -> ValidateOutput {
    let report = validate_text(text);
    ValidateOutput { valid: !report.has_errors(), problems: report.problems }
}
