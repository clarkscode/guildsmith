//! `RawPort` for the fake: canned responses by method and path, set by tests. Writes are
//! recorded with their audit reason; an unknown path is a 404.

use std::collections::BTreeMap;

use guildsmith_core::raw::{RawMethod, RawRequest};
use serde_json::Value;

use super::FakeDiscord;
use crate::ports::{PortError, PortResult, RawPort, RawReply};

/// Canned raw responses, keyed by (method, path without query).
#[derive(Debug, Default, Clone)]
pub(crate) struct RawRoutes(BTreeMap<(RawMethod, String), (u16, Value)>);

/// Every raw request the fake received, in order.
#[derive(Debug, Default, Clone)]
pub struct RawLog(pub Vec<RawRequest>);

impl FakeDiscord {
    /// Answers `method path` with `status` and `json`.
    pub fn set_raw_response(&self, method: RawMethod, path: &str, status: u16, json: Value) {
        self.lock().raw_routes.0.insert((method, path.trim_start_matches('/').to_owned()), (status, json));
    }

    /// Every raw request received so far.
    #[must_use]
    pub fn raw_requests(&self) -> Vec<RawRequest> {
        self.lock().raw_log.0.clone()
    }
}

/// What the fake answers by itself: the lookups the raw service makes (a channel's, webhook's
/// or invite's server).
fn builtin(st: &super::State, path: &str) -> Option<Value> {
    let segs: Vec<&str> = path.split('/').collect();
    let guild_with =
        |f: &dyn Fn(&super::FakeGuild) -> bool| st.guilds.values().find(|g| f(g)).map(|g| g.snapshot.guild.id);
    match segs.as_slice() {
        ["channels", c] => {
            let id: guildsmith_core::Id = c.parse().ok()?;
            let g = guild_with(&|g| {
                g.snapshot.channel(id).is_some()
                    || g.res.peek::<super::threads::ThreadsState>().is_some_and(|t| t.threads.contains_key(&id))
            })?;
            Some(serde_json::json!({"id": c, "guild_id": g.to_string()}))
        }
        ["webhooks", w] => {
            let id: guildsmith_core::Id = w.parse().ok()?;
            let g = guild_with(&|g| {
                g.res.peek::<super::webhooks::WebhooksState>().is_some_and(|s| s.hooks.contains_key(&id))
            })?;
            Some(serde_json::json!({"id": w, "guild_id": g.to_string(), "token": "fake-raw-webhook-token"}))
        }
        ["invites", code] => {
            let g = guild_with(&|g| {
                g.res.peek::<super::invites::InvitesState>().is_some_and(|s| s.invites.contains_key(*code))
            })?;
            Some(serde_json::json!({"code": code, "guild": {"id": g.to_string()}}))
        }
        _ => None,
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl RawPort for FakeDiscord {
    async fn raw_request(&self, req: &RawRequest, reason: Option<&str>) -> PortResult<RawReply> {
        if req.method.is_write() {
            self.record_write("raw_request", None, None, reason)?;
        } else {
            self.read("raw_request", |_| Ok(()))?;
        }
        let mut st = self.lock();
        st.raw_log.0.push(req.clone());
        let key = (req.method, req.path.trim_start_matches('/').to_owned());
        if !st.raw_routes.0.contains_key(&key)
            && req.method == RawMethod::Get
            && let Some(found) = builtin(&st, &key.1)
        {
            return Ok(RawReply { status: 200, json: found });
        }
        match st.raw_routes.0.get(&key) {
            Some((status, json)) if *status < 400 => Ok(RawReply { status: *status, json: json.clone() }),
            Some((status, json)) => Err(PortError::Discord {
                status: *status,
                code: json.get("code").and_then(Value::as_u64),
                message: json.get("message").and_then(Value::as_str).unwrap_or("error").to_owned(),
            }),
            None => Err(PortError::NotFound(format!("{} {}", req.method, req.path))),
        }
    }
}
