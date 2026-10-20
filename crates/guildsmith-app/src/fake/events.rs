//! `EventsPort` for the fake: at most 100 scheduled or active events, external events need a
//! place and an end, and only valid status changes (scheduled → active → completed, or
//! scheduled → canceled).

use std::collections::{BTreeMap, BTreeSet};

use guildsmith_core::{Id, Permission};

use super::assets::uploaded;
use super::{FakeDiscord, next_id, not_found};
use crate::ports::{EventFields, EventInfo, EventLocation, EventsPort, PortError, PortResult, UserInfo};

/// The fake's events for one server.
#[derive(Debug, Clone, Default)]
pub struct EventsState {
    pub events: BTreeMap<Id, EventInfo>,
    pub interested: BTreeMap<Id, BTreeSet<Id>>,
}

const STATUS: [&str; 5] = ["", "scheduled", "active", "completed", "canceled"];

fn bad(message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(50035), message: message.to_owned() }
}

fn check(e: &EventInfo) -> PortResult<()> {
    if let EventLocation::External(place) = &e.location
        && (place.trim().is_empty() || e.end.is_none())
    {
        return Err(bad("external events need a location and an end time"));
    }
    if e.end.as_ref().is_some_and(|end| *end <= e.start) {
        return Err(bad("the end must be after the start"));
    }
    Ok(())
}

impl FakeDiscord {
    /// Marks members as interested in an event.
    pub fn interested(&self, guild: Id, event: Id, users: &[Id]) {
        self.edit_res::<EventsState>(guild, |s| {
            let set = s.interested.entry(event).or_default();
            set.extend(users.iter().copied());
            let n = u32::try_from(set.len()).unwrap_or(u32::MAX);
            if let Some(e) = s.events.get_mut(&event) {
                e.interested = n;
            }
        });
    }
}

impl FakeDiscord {
    fn serve_cover(&self, e: &EventInfo, image: Option<(String, Vec<u8>)>) {
        if let Some((h, bytes)) = image {
            self.set_url(&guildsmith_core::live::event_cover_url(e.id, &h), bytes);
        }
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl EventsPort for FakeDiscord {
    async fn events(&self, guild: Id) -> PortResult<Vec<EventInfo>> {
        self.read_guild("events", guild, None, |g| {
            Ok(g.res.peek::<EventsState>().map(|s| s.events.values().cloned().collect()).unwrap_or_default())
        })
    }

    async fn create_event(&self, guild: Id, f: &EventFields, reason: &str) -> PortResult<EventInfo> {
        let image = f.image.as_deref().and_then(uploaded);
        let created = self.write("create_event", guild, None, Some(reason), |g, next| {
            g.bot_can(Permission::CreateEvents)?;
            let live = g
                .res
                .peek::<EventsState>()
                .map_or(0, |s| s.events.values().filter(|e| e.status == "scheduled" || e.status == "active").count());
            if live >= 100 {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(30038),
                    message: "Maximum number of guild scheduled events reached (100)".to_owned(),
                });
            }
            let e = EventInfo {
                id: next_id(next),
                name: f.name.clone().ok_or_else(|| bad("name is required"))?,
                description: f.description.clone().flatten(),
                start: f.start.clone().ok_or_else(|| bad("scheduled_start_time is required"))?,
                end: f.end.clone(),
                status: "scheduled".to_owned(),
                location: f.location.clone().ok_or_else(|| bad("a location is required"))?,
                interested: 0,
                creator: Some(g.snapshot.bot.user_id),
                recurrence: f.recurrence.clone(),
                image: image.as_ref().map(|(h, _)| h.clone()),
            };
            check(&e)?;
            g.res.get::<EventsState>().events.insert(e.id, e.clone());
            Ok(e)
        })?;
        self.serve_cover(&created, image);
        Ok(created)
    }

    async fn update_event(&self, guild: Id, event: Id, f: &EventFields, reason: &str) -> PortResult<EventInfo> {
        let image = f.image.as_deref().and_then(uploaded);
        let updated = self.write("update_event", guild, Some(event), Some(reason), |g, _| {
            g.bot_can(Permission::ManageEvents)?;
            let s = g.res.get::<EventsState>();
            let e = s.events.get_mut(&event).ok_or_else(|| not_found("event", event))?;
            let mut new = e.clone();
            if let Some(v) = &f.name {
                new.name.clone_from(v);
            }
            if let Some(v) = &f.description {
                new.description.clone_from(v);
            }
            if let Some(v) = &f.start {
                new.start.clone_from(v);
            }
            if let Some(v) = &f.end {
                new.end = Some(v.clone());
            }
            if let Some(v) = &f.location {
                new.location = v.clone();
            }
            if let Some(v) = &f.recurrence {
                new.recurrence = Some(v.clone());
            }
            if let Some((h, _)) = &image {
                new.image = Some(h.clone());
            }
            if let Some(st) = f.status {
                let to = STATUS.get(usize::from(st)).copied().unwrap_or_default();
                let ok =
                    matches!((e.status.as_str(), to), ("scheduled", "active" | "canceled") | ("active", "completed"));
                if !ok {
                    return Err(bad(&format!("an event can't go from {} to {to}", e.status)));
                }
                to.clone_into(&mut new.status);
            }
            check(&new)?;
            *e = new.clone();
            Ok(new)
        })?;
        self.serve_cover(&updated, image);
        Ok(updated)
    }

    async fn delete_event(&self, guild: Id, event: Id, reason: &str) -> PortResult<()> {
        self.write("delete_event", guild, Some(event), Some(reason), |g, _| {
            g.bot_can(Permission::ManageEvents)?;
            g.res.get::<EventsState>().events.remove(&event).map(|_| ()).ok_or_else(|| not_found("event", event))
        })
    }

    async fn event_users(&self, guild: Id, event: Id, after: Option<Id>, limit: u8) -> PortResult<Vec<UserInfo>> {
        self.read_guild("event_users", guild, Some(event), |g| {
            let s = g.res.peek::<EventsState>();
            if !s.is_some_and(|s| s.events.contains_key(&event)) {
                return Err(not_found("event", event));
            }
            let users = s.and_then(|s| s.interested.get(&event)).cloned().unwrap_or_default();
            Ok(users
                .into_iter()
                .filter(|u| after.is_none_or(|a| *u > a))
                .take(usize::from(limit))
                .map(|id| UserInfo {
                    id,
                    name: g
                        .members
                        .iter()
                        .find(|m| m.id == id)
                        .map_or_else(|| format!("user{id}"), |m| m.username.clone()),
                })
                .collect())
        })
    }
}
