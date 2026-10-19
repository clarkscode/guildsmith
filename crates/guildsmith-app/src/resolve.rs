//! Name resolution for tool and CLI inputs (data-model §6, FR-029).
//!
//! Accepts mention forms (`@Moderator`, `#general`, `<@&10>`, `<#101>`, `<@50>`), plain names,
//! `Category/channel` paths and ids. Ambiguous names return every candidate; missing names
//! return up to three close suggestions.

use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::{Id, suggest};
use serde::Serialize;

use crate::ports::Member;

/// One possible match for an ambiguous name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub name: String,
    pub id: Id,
    /// The category a channel is in, if any.
    pub parent: Option<String>,
}

/// The result of resolving a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Resolved(Id),
    Ambiguous(Vec<Candidate>),
    NotFound { suggestions: Vec<String> },
}

fn strip_wrapped<'a>(input: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let inner = input.strip_prefix('<')?.strip_suffix('>')?;
    prefixes.iter().find_map(|p| inner.strip_prefix(p))
}

fn from_matches(matches: Vec<Candidate>, input: &str, all: &[&str]) -> Resolution {
    match matches.len() {
        0 => Resolution::NotFound { suggestions: suggest::closest(input, all.iter().copied(), 3) },
        1 => Resolution::Resolved(matches[0].id),
        _ => Resolution::Ambiguous(matches),
    }
}

/// Resolves a role: `@Name`, `Name`, `<@&id>`, an id, or `@everyone`.
#[must_use]
pub fn resolve_role(snapshot: &Snapshot, input: &str) -> Resolution {
    let input = input.trim();
    if input == "@everyone" || input == "everyone" {
        return Resolution::Resolved(snapshot.guild.id);
    }
    let raw = strip_wrapped(input, &["@&"]).unwrap_or(input);
    if let Ok(id) = raw.parse::<Id>()
        && snapshot.role(id).is_some()
    {
        return Resolution::Resolved(id);
    }
    let name = raw.strip_prefix('@').unwrap_or(raw);
    let matches = snapshot
        .roles
        .iter()
        .filter(|r| r.id != snapshot.guild.id && r.name == name)
        .map(|r| Candidate { name: r.name.clone(), id: r.id, parent: None })
        .collect();
    let all: Vec<&str> = snapshot.roles.iter().filter(|r| r.id != snapshot.guild.id).map(|r| r.name.as_str()).collect();
    from_matches(matches, name, &all)
}

/// Resolves a channel or category: `#name`, `name`, `Category/name`, `<#id>`, or an id.
#[must_use]
pub fn resolve_channel(snapshot: &Snapshot, input: &str) -> Resolution {
    let input = input.trim();
    let raw = strip_wrapped(input, &["#"]).unwrap_or(input);
    if let Ok(id) = raw.parse::<Id>()
        && snapshot.channel(id).is_some()
    {
        return Resolution::Resolved(id);
    }
    let raw = raw.strip_prefix('#').unwrap_or(raw);
    let parent_name = |parent: Option<Id>| parent.and_then(|p| snapshot.channel(p)).map(|c| c.name.clone());
    let (category, name) = match raw.rsplit_once('/') {
        Some((cat, name)) => (Some(cat), name),
        None => (None, raw),
    };
    let matches = snapshot
        .channels
        .iter()
        .filter(|c| c.name == name)
        .filter(|c| match category {
            Some(cat) => parent_name(c.parent_id).as_deref() == Some(cat),
            None => true,
        })
        .map(|c| Candidate {
            name: c.name.clone(),
            id: c.id,
            parent: if c.kind == LiveChannelKind::Category { None } else { parent_name(c.parent_id) },
        })
        .collect();
    let all: Vec<&str> = snapshot.channels.iter().map(|c| c.name.as_str()).collect();
    from_matches(matches, name, &all)
}

/// Resolves a member: `@username`, a username, display name or nickname, `<@id>`, or an id.
#[must_use]
pub fn resolve_member(members: &[Member], input: &str) -> Resolution {
    let input = input.trim();
    let raw = strip_wrapped(input, &["@!", "@"]).unwrap_or(input);
    if let Ok(id) = raw.parse::<Id>()
        && members.iter().any(|m| m.id == id)
    {
        return Resolution::Resolved(id);
    }
    let name = raw.strip_prefix('@').unwrap_or(raw);
    let candidate = |m: &Member| Candidate { name: m.username.clone(), id: m.id, parent: None };
    // Usernames are unique, so an exact username wins outright.
    if let Some(m) = members.iter().find(|m| m.username == name) {
        return Resolution::Resolved(m.id);
    }
    let matches = members
        .iter()
        .filter(|m| m.nick.as_deref() == Some(name) || m.display_name.as_deref() == Some(name))
        .map(candidate)
        .collect();
    let all: Vec<&str> = members
        .iter()
        .flat_map(|m| [Some(m.username.as_str()), m.nick.as_deref(), m.display_name.as_deref()])
        .flatten()
        .collect();
    from_matches(matches, name, &all)
}
