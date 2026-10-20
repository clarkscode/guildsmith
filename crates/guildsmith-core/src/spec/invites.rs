//! `[[invites]]` (data-model §4). An invite without a `code` is matched by its signature.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_false(b: &bool) -> bool {
    !*b
}

/// Invites last at most 7 days, unless 0 (never expire).
pub const MAX_INVITE_AGE: u32 = 604_800;

pub const MAX_INVITE_USES: u32 = 100;

fn day() -> u32 {
    86_400
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_day(v: &u32) -> bool {
    *v == 86_400
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// `[[invites]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteSpec {
    /// Written back by apply. Without it, the invite is matched by its signature: channel,
    /// `max_age`, `max_uses`, temporary and roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The channel it opens on (`#name` or an id).
    pub channel: String,
    /// Seconds, 0–604800; 0 never expires. Default 86400 (a day).
    #[serde(default = "day", skip_serializing_if = "is_day")]
    pub max_age: u32,
    /// 0–100; 0 is unlimited.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_uses: u32,
    /// Members who join through it are kicked when they go offline, unless given a role.
    #[serde(default, skip_serializing_if = "is_false")]
    pub temporary: bool,
    /// Roles new members get (`@Role`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// Make a new invite when this one has expired or was deleted. Default false: report it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub recreate: bool,
}

impl InviteSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.max_age > MAX_INVITE_AGE {
            out.push("max_age is at most 604800 seconds (7 days); 0 never expires".to_owned());
        }
        if self.max_uses > MAX_INVITE_USES {
            out.push("max_uses is at most 100; 0 is unlimited".to_owned());
        }
        out
    }
}
