//! Moderation endpoints sent raw: prune (twilight's `GuildPrune` can't hold the `null` count
//! that `compute_prune_count = false` returns), the audit log (only the fields guildsmith
//! reports), and incident actions (missing in twilight 0.17.1).

use guildsmith_app::ports::{AuditEntry, Incidents, PortResult};
use guildsmith_core::Id;
use serde::{Deserialize, Serialize};
use twilight_http::Client;
use twilight_http::request::Method;

use crate::raw;

#[derive(Debug, Deserialize)]
struct PruneResponse {
    pruned: Option<u64>,
}

fn roles_query(days: u8, roles: &[Id]) -> Vec<(String, String)> {
    let mut q = vec![("days".to_owned(), days.to_string())];
    if !roles.is_empty() {
        q.push(("include_roles".to_owned(), roles.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")));
    }
    q
}

pub(crate) async fn prune_count(http: &Client, guild: Id, days: u8, roles: &[Id]) -> PortResult<u64> {
    let res: PruneResponse =
        raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/prune"), &roles_query(days, roles), None, None)
            .await?;
    Ok(res.pruned.unwrap_or(0))
}

#[derive(Debug, Serialize)]
struct PruneBody<'a> {
    days: u8,
    compute_prune_count: bool,
    #[serde(skip_serializing_if = "<[Id]>::is_empty")]
    include_roles: &'a [Id],
}

pub(crate) async fn prune(
    http: &Client,
    guild: Id,
    days: u8,
    roles: &[Id],
    compute_count: bool,
    reason: &str,
) -> PortResult<Option<u64>> {
    let body = PruneBody { days, compute_prune_count: compute_count, include_roles: roles };
    let res: PruneResponse =
        raw::json(http, Method::Post, &format!("guilds/{guild}/prune"), &[], Some(&body), Some(reason)).await?;
    Ok(res.pruned)
}

#[derive(Debug, Deserialize)]
struct AuditLog {
    audit_log_entries: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    id: Id,
    action_type: u16,
    user_id: Option<Id>,
    target_id: Option<Id>,
    reason: Option<String>,
    #[serde(default)]
    changes: Vec<RawChange>,
}

#[derive(Debug, Deserialize)]
struct RawChange {
    key: String,
}

pub(crate) async fn audit_log(
    http: &Client,
    guild: Id,
    action_type: Option<u16>,
    user: Option<Id>,
    before: Option<Id>,
    limit: u8,
) -> PortResult<Vec<AuditEntry>> {
    let mut q = vec![("limit".to_owned(), limit.to_string())];
    if let Some(t) = action_type {
        q.push(("action_type".to_owned(), t.to_string()));
    }
    if let Some(u) = user {
        q.push(("user_id".to_owned(), u.to_string()));
    }
    if let Some(b) = before {
        q.push(("before".to_owned(), b.to_string()));
    }
    let log: AuditLog =
        raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/audit-logs"), &q, None, None).await?;
    Ok(log
        .audit_log_entries
        .into_iter()
        .map(|e| AuditEntry {
            id: e.id,
            action_type: e.action_type,
            user_id: e.user_id,
            target_id: e.target_id,
            reason: e.reason,
            changes: e.changes.into_iter().map(|c| c.key).collect(),
        })
        .collect())
}

/// `PUT guilds/{g}/incident-actions`: both times are sent; `null` turns one off.
#[derive(Debug, Serialize, Deserialize)]
struct IncidentBody {
    invites_disabled_until: Option<String>,
    dms_disabled_until: Option<String>,
}

pub(crate) async fn set_incidents(http: &Client, guild: Id, i: &Incidents, reason: &str) -> PortResult<Incidents> {
    let body = IncidentBody {
        invites_disabled_until: i.invites_disabled_until.clone(),
        dms_disabled_until: i.dms_disabled_until.clone(),
    };
    let res: IncidentBody =
        raw::json(http, Method::Put, &format!("guilds/{guild}/incident-actions"), &[], Some(&body), Some(reason))
            .await?;
    Ok(Incidents { invites_disabled_until: res.invites_disabled_until, dms_disabled_until: res.dms_disabled_until })
}
