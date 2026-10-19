//! Which servers guildsmith may touch, and resolving `-g`/`guild` inputs.
//!
//! The allowlist filtering lives here, not in the interfaces (constitution I/II).

use guildsmith_core::Id;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::{DiscordPort, GuildSummary};
use crate::resolve::Candidate;

/// One server in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct GuildEntry {
    pub id: Id,
    pub name: String,
    pub allowed: bool,
}

/// The servers the bot can access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct GuildList {
    pub guilds: Vec<GuildEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub const NO_SERVERS_HINT: &str = "invite the bot to a server you created; bots can't create servers";

/// Lists servers. Servers outside the allowlist are hidden unless `include_disallowed`.
pub async fn list<P: DiscordPort>(app: &App<P>, include_disallowed: bool) -> Result<GuildList, ServiceError> {
    let all = app.port.list_guilds().await?;
    let guilds: Vec<GuildEntry> = all
        .into_iter()
        .map(|g| GuildEntry { allowed: app.policy.is_allowed(g.id), id: g.id, name: g.name })
        .filter(|g| include_disallowed || g.allowed)
        .collect();
    let hint = guilds.is_empty().then(|| NO_SERVERS_HINT.to_owned());
    Ok(GuildList { guilds, hint })
}

/// Resolves a server input (an id or exact name) to an allowed server id.
///
/// With no input, the single allowed server is used when there is exactly one.
pub async fn resolve<P: DiscordPort>(app: &App<P>, input: Option<&str>) -> Result<Id, ServiceError> {
    let input = input.map(str::trim).filter(|s| !s.is_empty());
    // An id that's on the allowlist needs no lookup; anything else checks the bot's servers.
    if let Some(id) = input.and_then(|s| s.parse::<Id>().ok()) {
        app.policy.ensure_allowed(id)?;
        return Ok(id);
    }
    if input.is_none()
        && let Some(list) = &app.policy.allowlist
        && list.len() == 1
    {
        return Ok(*list.iter().next().expect("one entry"));
    }
    let all: Vec<GuildSummary> = app.port.list_guilds().await?;
    let Some(name) = input else {
        let allowed: Vec<_> = all.iter().filter(|g| app.policy.is_allowed(g.id)).collect();
        return match allowed.as_slice() {
            [one] => Ok(one.id),
            [] => Err(ServiceError::Invalid(format!("no server is available; {NO_SERVERS_HINT}"))),
            _ => Err(ServiceError::Invalid(
                "more than one server is available; pass a server id or name (-g / guild)".to_owned(),
            )),
        };
    };
    let matches: Vec<&GuildSummary> = all.iter().filter(|g| g.name == name).collect();
    match matches.as_slice() {
        [one] => {
            app.policy.ensure_allowed(one.id)?;
            Ok(one.id)
        }
        [] => Err(ServiceError::NotFound {
            what: "server",
            input: name.to_owned(),
            suggestions: guildsmith_core::suggest::closest(name, all.iter().map(|g| g.name.as_str()), 3),
        }),
        many => Err(ServiceError::Ambiguous {
            what: "server",
            input: name.to_owned(),
            candidates: many.iter().map(|g| Candidate { name: g.name.clone(), id: g.id, parent: None }).collect(),
        }),
    }
}
