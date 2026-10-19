//! The safety policy both interfaces call (constitution I): allowlist, read-only, toolsets.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use guildsmith_core::Id;
use serde::Serialize;

/// A named set of agent tools. Only `core` is on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Toolset {
    Core,
    Moderation,
}

impl Toolset {
    pub const ALL: &'static [Toolset] = &[Toolset::Core, Toolset::Moderation];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Toolset::Core => "core",
            Toolset::Moderation => "moderation",
        }
    }
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An unknown toolset name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown toolset `{name}`; valid toolsets: {valid}")]
pub struct UnknownToolset {
    pub name: String,
    pub valid: String,
}

impl FromStr for Toolset {
    type Err = UnknownToolset;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Toolset::ALL.iter().copied().find(|t| t.name() == s).ok_or_else(|| UnknownToolset {
            name: s.to_owned(),
            valid: Toolset::ALL.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "),
        })
    }
}

/// Parses `GUILDSMITH_TOOLSETS` (`core,moderation`, `default`). `core` is always included.
pub fn parse_toolsets(raw: &str) -> Result<BTreeSet<Toolset>, UnknownToolset> {
    let mut set = BTreeSet::from([Toolset::Core]);
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if part == "default" {
            continue;
        }
        set.insert(part.parse()?);
    }
    Ok(set)
}

/// Parses `GUILDSMITH_GUILDS`: comma-separated server ids.
pub fn parse_allowlist(raw: &str) -> Result<BTreeSet<Id>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<Id>().map_err(|e| format!("GUILDSMITH_GUILDS: {e}")))
        .collect()
}

/// Why a request was refused. Interfaces show this as exit code 3 or an MCP `refusal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind {
    NotAllowed,
    ReadOnly,
    NeedsConfirmation,
    PlanMismatch,
    Preflight,
    NotBotToken,
}

/// A policy refusal: nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{reason}")]
pub struct Refusal {
    pub kind: RefusalKind,
    pub reason: String,
    pub hint: Option<String>,
    /// Structured specifics, e.g. every preflight problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl Refusal {
    pub fn new(kind: RefusalKind, reason: impl Into<String>, hint: Option<&str>) -> Self {
        Refusal { kind, reason: reason.into(), hint: hint.map(str::to_owned), details: None }
    }

    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// The operator's rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `None` = every server the bot is in (with a startup warning).
    pub allowlist: Option<BTreeSet<Id>>,
    pub read_only: bool,
    pub toolsets: BTreeSet<Toolset>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { allowlist: None, read_only: false, toolsets: BTreeSet::from([Toolset::Core]) }
    }
}

impl Policy {
    /// Refuses servers outside the allowlist.
    pub fn ensure_allowed(&self, guild: Id) -> Result<(), Refusal> {
        match &self.allowlist {
            Some(list) if !list.contains(&guild) => Err(Refusal::new(
                RefusalKind::NotAllowed,
                format!("server {guild} is not on the allowlist"),
                Some("add its id to GUILDSMITH_GUILDS to allow it"),
            )),
            _ => Ok(()),
        }
    }

    #[must_use]
    pub fn is_allowed(&self, guild: Id) -> bool {
        self.ensure_allowed(guild).is_ok()
    }

    /// Refuses every write in read-only mode.
    pub fn ensure_writable(&self) -> Result<(), Refusal> {
        if self.read_only {
            Err(Refusal::new(
                RefusalKind::ReadOnly,
                "read-only mode is on; nothing was changed",
                Some("unset GUILDSMITH_READ_ONLY or drop --read-only to allow changes"),
            ))
        } else {
            Ok(())
        }
    }

    /// The startup warning for an empty allowlist (FR-016).
    #[must_use]
    pub fn allowlist_warning(&self) -> Option<&'static str> {
        self.allowlist
            .is_none()
            .then_some("no allowlist set; all servers the bot is in are reachable (set GUILDSMITH_GUILDS)")
    }
}

/// Builds the audit-log reason for a write: `guildsmith: <summary>`, at most 512 characters.
#[must_use]
pub fn audit_reason(summary: &str, extra: Option<&str>) -> String {
    let mut reason = format!("guildsmith: {summary}");
    if let Some(extra) = extra.filter(|e| !e.trim().is_empty()) {
        reason.push_str(" (");
        reason.push_str(extra.trim());
        reason.push(')');
    }
    if reason.chars().count() > 512 {
        reason = reason.chars().take(511).collect::<String>() + "…";
    }
    reason
}
