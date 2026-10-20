//! Scheduled event create/update bodies with `recurrence_rule` (missing in twilight 0.17.1), and
//! Discord's event JSON.

use guildsmith_app::ports::{EventFields, EventInfo, EventLocation, PortResult};
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Deserialize)]
struct Metadata {
    #[serde(default)]
    location: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEvent {
    id: Id,
    #[serde(default)]
    channel_id: Option<Id>,
    #[serde(default)]
    creator_id: Option<Id>,
    name: String,
    #[serde(default)]
    description: Option<String>,
    scheduled_start_time: String,
    #[serde(default)]
    scheduled_end_time: Option<String>,
    status: u8,
    entity_type: u8,
    #[serde(default)]
    entity_metadata: Option<Metadata>,
    #[serde(default)]
    user_count: Option<u32>,
    #[serde(default)]
    recurrence_rule: Option<Value>,
    #[serde(default)]
    image: Option<String>,
}

impl From<RawEvent> for EventInfo {
    fn from(e: RawEvent) -> Self {
        let location = match (e.entity_type, e.channel_id) {
            (1, Some(c)) => EventLocation::Stage(c),
            (2, Some(c)) => EventLocation::Voice(c),
            _ => EventLocation::External(e.entity_metadata.and_then(|m| m.location).unwrap_or_default()),
        };
        EventInfo {
            id: e.id,
            name: e.name,
            description: e.description.filter(|d| !d.is_empty()),
            start: e.scheduled_start_time,
            end: e.scheduled_end_time,
            status: ["", "scheduled", "active", "completed", "canceled"]
                .get(usize::from(e.status))
                .copied()
                .unwrap_or("unknown")
                .to_owned(),
            location,
            interested: e.user_count.unwrap_or(0),
            creator: e.creator_id,
            recurrence: e.recurrence_rule.filter(|r| !r.is_null()),
            image: e.image,
        }
    }
}

/// The JSON body for creating (all given fields) or updating (only set fields) an event.
pub(crate) fn body(f: &EventFields, create: bool) -> Value {
    let mut b = Map::new();
    if create {
        b.insert("privacy_level".to_owned(), json!(2));
    }
    if let Some(v) = &f.name {
        b.insert("name".to_owned(), json!(v));
    }
    if let Some(v) = &f.description {
        b.insert("description".to_owned(), json!(v));
    }
    if let Some(v) = &f.start {
        b.insert("scheduled_start_time".to_owned(), json!(v));
    }
    if let Some(v) = &f.end {
        b.insert("scheduled_end_time".to_owned(), json!(v));
    }
    match &f.location {
        Some(EventLocation::Stage(c)) => {
            b.insert("entity_type".to_owned(), json!(1));
            b.insert("channel_id".to_owned(), json!(c));
            if !create {
                b.insert("entity_metadata".to_owned(), Value::Null);
            }
        }
        Some(EventLocation::Voice(c)) => {
            b.insert("entity_type".to_owned(), json!(2));
            b.insert("channel_id".to_owned(), json!(c));
            if !create {
                b.insert("entity_metadata".to_owned(), Value::Null);
            }
        }
        Some(EventLocation::External(place)) => {
            b.insert("entity_type".to_owned(), json!(3));
            b.insert("entity_metadata".to_owned(), json!({"location": place}));
            b.insert("channel_id".to_owned(), Value::Null);
        }
        None => {}
    }
    if let Some(v) = f.status {
        b.insert("status".to_owned(), json!(v));
    }
    if let Some(v) = &f.recurrence {
        b.insert("recurrence_rule".to_owned(), v.clone());
    }
    if let Some(v) = &f.image {
        b.insert("image".to_owned(), json!(v));
    }
    Value::Object(b)
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEventUser {
    pub user: crate::messages::RawUser,
}

/// Parses an event response.
pub(crate) fn event(v: Value) -> PortResult<EventInfo> {
    serde_json::from_value::<RawEvent>(v).map(EventInfo::from).map_err(|e| guildsmith_app::ports::PortError::Discord {
        status: 0,
        code: None,
        message: format!("unexpected event: {e}"),
    })
}
