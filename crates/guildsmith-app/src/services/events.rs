//! The `events` group (US3, FR-025): scheduled events with a voice, stage or external location,
//! time zones, recurrence, and interested users.
//!
//! Times take an offset (in the time or as `tz`); a time without one is UTC, and results show
//! times in UTC with "UTC" (spec edge cases). Names and descriptions are user-written: tools
//! return them wrapped as untrusted.

use guildsmith_core::Id;
use guildsmith_core::asset::{Asset, AssetKind};
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::spec::ChannelKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::App;
use crate::assets::{self, Caller};
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, EventFields, EventInfo, EventLocation, UserInfo};
use crate::preflight;
use crate::resolve;
use crate::services::live::{Deleted, invalid, refuse_if, resolved, server, writable};

pub use guildsmith_core::spec::events::{NWeekday, Recurrence as RecurrenceInput, WEEKDAYS};

/// Discord's `recurrence_rule` for `r`, starting at `start`.
pub fn recurrence_rule(r: &RecurrenceInput, start: &str) -> Result<Value, ServiceError> {
    guildsmith_core::spec::events::recurrence_rule(r, start).map_err(invalid)
}

/// At most this many scheduled or active events per server.
pub const MAX_EVENTS: usize = 100;

// ---- times -------------------------------------------------------------------------------------

/// `+HH:MM` / `-HH:MM` / `UTC` / `Z` → seconds east of UTC.
fn offset_secs(tz: &str) -> Result<i64, ServiceError> {
    let t = tz.trim();
    if t.eq_ignore_ascii_case("utc") || t == "Z" || t.eq_ignore_ascii_case("gmt") {
        return Ok(0);
    }
    let bad = || {
        invalid(format!(
            "time zone `{tz}` isn't an offset; give it like -07:00 or +02:00 (names like Europe/Paris aren't supported)"
        ))
    };
    let sign = match t.chars().next() {
        Some('+') => 1,
        Some('-') => -1,
        _ => return Err(bad()),
    };
    let (h, m) = t[1..].split_once(':').unwrap_or((&t[1..], "0"));
    let (h, m): (i64, i64) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if h > 14 || m > 59 {
        return Err(bad());
    }
    Ok(sign * (h * 3600 + m * 60))
}

/// A time with an offset, or a local time read in `tz` (UTC when none). Returns Unix seconds.
pub fn event_time(input: &str, tz: Option<&str>) -> Result<i64, ServiceError> {
    if let Some(t) = date::parse_rfc3339(input) {
        if tz.is_some() {
            return Err(invalid(format!("`{input}` already has an offset; leave out tz")));
        }
        return Ok(t);
    }
    let mut local = input.trim().replace(' ', "T");
    if local.len() == 16 {
        local.push_str(":00");
    }
    let utc = date::parse_rfc3339(&format!("{local}Z"))
        .ok_or_else(|| invalid(format!("`{input}` isn't a time like 2026-10-23T20:00 or 2026-10-23T20:00-07:00")))?;
    Ok(utc - tz.map(offset_secs).transpose()?.unwrap_or(0))
}

fn utc(secs: i64) -> String {
    date::rfc3339(u64::try_from(secs).unwrap_or(0))
}

/// `2026-10-23 20:00 UTC`.
fn shown(rfc: &str) -> String {
    date::parse_rfc3339(rfc).map_or_else(
        || rfc.to_owned(),
        |t| {
            let s = utc(t);
            format!("{} {} UTC", &s[..10], &s[11..16])
        },
    )
}

// ---- rows --------------------------------------------------------------------------------------

/// An event as tools return it. `name` and `description` are user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EventRow {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    /// e.g. `2026-10-23 20:00 UTC – 22:00 UTC`.
    pub when: String,
    /// RFC 3339 (UTC).
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// `voice #Lobby`, `stage Town Hall` or `external: <place>`.
    pub location: String,
    pub interested: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeats: Option<String>,
}

fn location_label(s: &Snapshot, l: &EventLocation) -> String {
    let name = |id: &Id| s.channel(*id).map_or_else(|| id.to_string(), |c| c.name.clone());
    match l {
        EventLocation::Voice(c) => format!("voice {}", name(c)),
        EventLocation::Stage(c) => format!("stage {}", name(c)),
        EventLocation::External(p) => format!("external: {p}"),
    }
}

fn repeats(r: &Value) -> String {
    let freq = ["yearly", "monthly", "weekly", "daily"]
        .get(usize::try_from(r["frequency"].as_u64().unwrap_or(9)).unwrap_or(9))
        .copied()
        .unwrap_or("custom");
    let interval = r["interval"].as_u64().unwrap_or(1);
    let days: Vec<&str> = r["by_weekday"]
        .as_array()
        .map(|d| d.iter().filter_map(|x| WEEKDAYS.get(usize::try_from(x.as_u64()?).ok()?).copied()).collect())
        .unwrap_or_default();
    let every = if interval > 1 { format!("every {interval} weeks") } else { freq.to_owned() };
    if days.is_empty() { every } else { format!("{every} on {}", days.join(", ")) }
}

fn row(s: &Snapshot, e: &EventInfo) -> EventRow {
    let when = match &e.end {
        Some(end) => format!("{} – {}", shown(&e.start), shown(end)),
        None => shown(&e.start),
    };
    EventRow {
        id: e.id,
        name: e.name.clone(),
        description: e.description.clone(),
        status: e.status.clone(),
        when,
        start: e.start.clone(),
        end: e.end.clone(),
        location: location_label(s, &e.location),
        interested: e.interested,
        repeats: e.recurrence.as_ref().map(repeats),
    }
}

// ---- inputs --------------------------------------------------------------------------------------

/// Where an event happens, as the tools take it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationInput {
    /// A voice channel.
    Voice(String),
    /// A stage channel.
    Stage(String),
    /// A place outside Discord (needs an end time).
    External(String),
}

/// An event to create, or changes to one (`None` leaves a field alone).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    /// An offset for times without one; UTC when none.
    pub tz: Option<String>,
    pub location: Option<LocationInput>,
    pub recurrence: Option<RecurrenceInput>,
    pub image: Option<Asset>,
}

fn location<P: DiscordPort>(_app: &App<P>, s: &Snapshot, l: &LocationInput) -> Result<EventLocation, ServiceError> {
    let channel = |input: &str, want: ChannelKind| {
        let id = resolved(resolve::resolve_channel(s, input), "channel", input)?;
        match s.channel(id).map(|c| c.kind) {
            Some(LiveChannelKind::Known(k)) if k == want => Ok(id),
            _ => Err(invalid(format!("`{input}` isn't a {} channel", want.as_str()))),
        }
    };
    Ok(match l {
        LocationInput::Voice(c) => EventLocation::Voice(channel(c, ChannelKind::Voice)?),
        LocationInput::Stage(c) => EventLocation::Stage(channel(c, ChannelKind::Stage)?),
        LocationInput::External(p) if p.trim().is_empty() || p.chars().count() > 100 => {
            return Err(invalid("an external location is 1–100 characters"));
        }
        LocationInput::External(p) => EventLocation::External(p.trim().to_owned()),
    })
}

async fn fields<P: DiscordPort>(
    app: &App<P>,
    s: &Snapshot,
    input: &EventInput,
    current: Option<&EventInfo>,
) -> Result<EventFields, ServiceError> {
    let now = date::now_secs(app.clock.as_ref());
    let tz = input.tz.as_deref();
    if let Some(n) = &input.name
        && !(1..=100).contains(&n.trim().chars().count())
    {
        return Err(invalid("event names are 1–100 characters"));
    }
    if input.description.as_ref().is_some_and(|d| d.chars().count() > 1000) {
        return Err(invalid("the description is at most 1000 characters"));
    }
    let start = input.start.as_deref().map(|t| event_time(t, tz)).transpose()?;
    let end = input.end.as_deref().map(|t| event_time(t, tz)).transpose()?;
    if start.is_some_and(|t| t <= now) {
        return Err(invalid("the start time must be in the future"));
    }
    let start_at = start.or_else(|| current.and_then(|c| date::parse_rfc3339(&c.start)));
    let end_at = end.or_else(|| current.and_then(|c| c.end.as_deref()).and_then(date::parse_rfc3339));
    if let (Some(a), Some(b)) = (start_at, end_at)
        && b <= a
    {
        return Err(invalid("the end must be after the start"));
    }
    let loc = input.location.as_ref().map(|l| location(app, s, l)).transpose()?;
    let effective = loc.as_ref().or(current.map(|c| &c.location));
    if matches!(effective, Some(EventLocation::External(_))) && end_at.is_none() {
        return Err(invalid("an external event needs an end time"));
    }
    let recurrence = match &input.recurrence {
        Some(r) => {
            let at = start_at.ok_or_else(|| invalid("a recurrence needs the start time"))?;
            Some(recurrence_rule(r, &utc(at))?)
        }
        None => None,
    };
    let image = match &input.image {
        Some(a) => {
            let l = assets::load(&app.port, &app.policy, Caller::Mcp { base: None }, a, AssetKind::GuildImage).await?;
            Some(guildsmith_core::asset::data_uri(l.file_type, &l.bytes))
        }
        None => None,
    };
    Ok(EventFields {
        name: input.name.as_ref().map(|n| n.trim().to_owned()),
        description: input.description.clone().map(|d| (!d.is_empty()).then_some(d)),
        start: start.map(utc),
        end: end.map(utc),
        location: loc,
        status: None,
        recurrence,
        image,
    })
}

// ---- tools ---------------------------------------------------------------------------------------

/// Schedules an event in a voice or stage channel, or at an external place (which needs an
/// end). At most 100 scheduled or active events per server.
pub async fn event_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    input: &EventInput,
    reason: Option<&str>,
) -> Result<EventRow, ServiceError> {
    if input.name.is_none() || input.start.is_none() || input.location.is_none() {
        return Err(invalid("an event needs name, start and location"));
    }
    let (gid, s) = writable(app, guild).await?;
    let f = fields(app, &s, input, None).await?;
    refuse_if(&preflight::check_tools(&s, ["event_create"]))?;
    let live = app.port.events(gid).await?.iter().filter(|e| e.status == "scheduled" || e.status == "active").count();
    if live >= MAX_EVENTS {
        return Err(invalid("this server already has 100 scheduled or active events, Discord's limit"));
    }
    let name = f.name.clone().unwrap_or_default();
    let e = app.port.create_event(gid, &f, &audit_reason(&format!("schedule event {name}"), reason)).await?;
    Ok(row(&s, &e))
}

async fn find_event<P: DiscordPort>(app: &App<P>, gid: Id, input: &str) -> Result<EventInfo, ServiceError> {
    let all = app.port.events(gid).await?;
    let raw = input.trim();
    if let Ok(id) = raw.parse::<Id>()
        && let Some(e) = all.iter().find(|e| e.id == id)
    {
        return Ok(e.clone());
    }
    let hits: Vec<&EventInfo> = all.iter().filter(|e| e.name == raw).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(ServiceError::NotFound {
            what: "event",
            input: input.to_owned(),
            suggestions: guildsmith_core::suggest::closest(raw, all.iter().map(|e| e.name.as_str()), 3),
        }),
        many => Err(ServiceError::Ambiguous {
            what: "event",
            input: input.to_owned(),
            candidates: many
                .iter()
                .map(|e| resolve::Candidate { name: format!("{} at {}", e.name, e.start), id: e.id, parent: None })
                .collect(),
        }),
    }
}

/// A status change for `event_update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Active,
    Completed,
    Canceled,
}

/// Changes an event, or starts, ends or cancels it (canceling needs confirmation). While an
/// event is active only its description, end and status can change.
pub async fn event_update<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    event: &str,
    input: &EventInput,
    status: Option<EventStatus>,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<EventRow>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let current = find_event(app, gid, event).await?;
    if matches!(current.status.as_str(), "completed" | "canceled") {
        return Err(invalid(format!("this event is {}; it can't change", current.status)));
    }
    if current.status == "active"
        && (input.name.is_some() || input.start.is_some() || input.location.is_some() || input.recurrence.is_some())
    {
        return Err(invalid("an active event can only change its description, end time and status"));
    }
    let mut f = fields(app, &s, input, Some(&current)).await?;
    f.status = status.map(|st| match st {
        EventStatus::Active => 2,
        EventStatus::Completed => 3,
        EventStatus::Canceled => 4,
    });
    refuse_if(&preflight::check_tools(&s, ["event_update"]))?;
    let summary = format!("update event {} ({})", current.name, current.id);
    let run = || async {
        let e = app.port.update_event(gid, current.id, &f, &audit_reason(&summary, reason)).await?;
        Ok(row(&s, &e))
    };
    if status == Some(EventStatus::Canceled) {
        let would = vec![format!(
            "cancel the event {} ({}); {} interested member(s) are told",
            current.name, current.id, current.interested
        )];
        confirm::guarded(confirmed, would, run).await
    } else {
        run().await.map(Outcome::Done)
    }
}

/// Deletes an event after confirmation.
pub async fn event_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    event: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let e = find_event(app, gid, event).await?;
    let what = format!("event {} ({})", e.name, e.id);
    confirm::guarded(confirmed, vec![format!("delete {what}")], || async {
        refuse_if(&preflight::check_tools(&s, ["event_delete"]))?;
        app.port.delete_event(gid, e.id, &audit_reason(&format!("delete event {}", e.id), reason)).await?;
        Ok(Deleted { deleted: format!("event {}", e.id) })
    })
    .await
}

/// Every event with its status and interested count, soonest first.
pub async fn events_list<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<Vec<EventRow>, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let mut all = app.port.events(gid).await?;
    all.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(all.iter().map(|e| row(&s, e)).collect())
}

/// Members interested in an event, in pages after a user id (`limit` ≤ 100, default 50).
pub async fn event_users<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    event: &str,
    limit: Option<u8>,
    after: Option<Id>,
) -> Result<Vec<UserInfo>, ServiceError> {
    let (gid, _) = server(app, guild).await?;
    let e = find_event(app, gid, event).await?;
    Ok(app.port.event_users(gid, e.id, after, limit.unwrap_or(50).clamp(1, 100)).await?)
}
