//! `[[events]]` (data-model §4) and the recurrence rules the `events` tools share.
//!
//! Times are TOML datetimes **with** a UTC offset; a local time is an error, so no time zone is
//! ever guessed.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::asset::Asset;
use crate::id::Id;
use crate::time;

pub const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// Names are 1–100 characters, descriptions at most 1000.
pub const MAX_NAME: usize = 100;
pub const MAX_DESCRIPTION: usize = 1000;

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::One(s)) => vec![s],
        Some(OneOrMany::Many(v)) => v,
    })
}

fn one_or_many_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "anyOf": [{ "type": "string" }, { "type": "array", "items": { "type": "string" } }],
        "description": "Weekly: one day; daily: a set of days (e.g. weekdays)."
    })
}

/// How an event repeats (Discord's limits: no count or end, one by-* entry, an interval only
/// for weekly events).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    /// daily, weekly, monthly or yearly.
    pub frequency: String,
    /// Weekly only: 1 or 2 (every other week).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<u8>,
    /// Weekly: one day; daily: a set of days (e.g. weekdays).
    #[serde(default, deserialize_with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    #[schemars(schema_with = "one_or_many_schema")]
    pub weekday: Vec<String>,
    /// Monthly: the nth weekday, e.g. {n: 2, day: "tuesday"}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_weekday: Option<NWeekday>,
    /// Yearly: the month (1-12) and day of the month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month_day: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NWeekday {
    /// 1-5.
    pub n: u8,
    pub day: String,
}

fn weekday(d: &str) -> Result<u8, String> {
    WEEKDAYS
        .iter()
        .position(|w| w.eq_ignore_ascii_case(d.trim()) || w[..3].eq_ignore_ascii_case(d.trim()))
        .map(|i| u8::try_from(i).unwrap_or(0))
        .ok_or_else(|| format!("`{d}` isn't a weekday"))
}

/// Discord's `recurrence_rule` for `r`, starting at `start` (RFC 3339).
pub fn recurrence_rule(r: &Recurrence, start: &str) -> Result<Value, String> {
    let freq = match r.frequency.to_ascii_lowercase().as_str() {
        "yearly" => 0,
        "monthly" => 1,
        "weekly" => 2,
        "daily" => 3,
        other => return Err(format!("frequency `{other}` isn't daily, weekly, monthly or yearly")),
    };
    let by =
        usize::from(!r.weekday.is_empty()) + usize::from(r.n_weekday.is_some()) + usize::from(r.month_day.is_some());
    if by > 1 {
        return Err("a recurrence takes one of weekday, n_weekday or month+month_day".to_owned());
    }
    if r.interval.is_some_and(|i| i != 1) && (freq != 2 || r.interval.is_some_and(|i| i > 2)) {
        return Err("interval is only for weekly events, and is 1 or 2 (every other week)".to_owned());
    }
    let mut rule = json!({"start": start, "frequency": freq, "interval": r.interval.unwrap_or(1)});
    match freq {
        2 => {
            let [day] = r.weekday.as_slice() else {
                return Err("a weekly event repeats on one weekday".to_owned());
            };
            rule["by_weekday"] = json!([weekday(day)?]);
        }
        3 if !r.weekday.is_empty() => {
            let days: Vec<u8> = r.weekday.iter().map(|d| weekday(d)).collect::<Result<_, _>>()?;
            rule["by_weekday"] = json!(days);
        }
        1 => {
            let nw = r.n_weekday.as_ref().ok_or("a monthly event repeats on an n_weekday (e.g. the 2nd Tuesday)")?;
            if !(1..=5).contains(&nw.n) {
                return Err("n_weekday.n is 1–5".to_owned());
            }
            rule["by_n_weekday"] = json!([{"n": nw.n, "day": weekday(&nw.day)?}]);
        }
        0 => {
            let (Some(m), Some(d)) = (r.month, r.month_day) else {
                return Err("a yearly event repeats on a month and month_day".to_owned());
            };
            if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
                return Err("month is 1–12 and month_day 1–31".to_owned());
            }
            rule["by_month"] = json!([m]);
            rule["by_month_day"] = json!([d]);
        }
        _ => {}
    }
    Ok(rule)
}

/// Keys of a `recurrence_rule` that say how an event repeats (Discord adds others, as null).
const RULE_KEYS: [&str; 6] = ["frequency", "interval", "by_weekday", "by_n_weekday", "by_month", "by_month_day"];

/// Whether two `recurrence_rule`s repeat the same way.
#[must_use]
pub fn same_rule(a: &Value, b: &Value) -> bool {
    let norm = |v: &Value, k: &str| match &v[k] {
        Value::Null => None,
        Value::Array(a) if a.is_empty() => None,
        x => Some(x.clone()),
    };
    RULE_KEYS.iter().all(|k| {
        let (x, y) = (norm(a, k), norm(b, k));
        // A missing interval is 1.
        if *k == "interval" { x.unwrap_or(json!(1)) == y.unwrap_or(json!(1)) } else { x == y }
    })
}

/// The repeat rule a live `recurrence_rule` describes, for export.
#[must_use]
pub fn from_rule(r: &Value) -> Option<Recurrence> {
    let frequency = ["yearly", "monthly", "weekly", "daily"].get(usize::try_from(r["frequency"].as_u64()?).ok()?)?;
    let day = |v: &Value| WEEKDAYS.get(usize::try_from(v.as_u64()?).ok()?).map(|d| (*d).to_owned());
    let interval = r["interval"].as_u64().and_then(|i| u8::try_from(i).ok()).filter(|i| *i != 1);
    let weekday = r["by_weekday"].as_array().map(|d| d.iter().filter_map(day).collect()).unwrap_or_default();
    let n_weekday = r["by_n_weekday"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|nw| Some(NWeekday { n: u8::try_from(nw["n"].as_u64()?).ok()?, day: day(&nw["day"])? }));
    let first =
        |k: &str| r[k].as_array().and_then(|a| a.first()).and_then(Value::as_u64).and_then(|v| u8::try_from(v).ok());
    Some(Recurrence {
        frequency: (*frequency).to_owned(),
        interval,
        weekday,
        n_weekday,
        month: first("by_month"),
        month_day: first("by_month_day"),
    })
}

/// A point in time with an explicit UTC offset, written as a TOML datetime
/// (`2026-10-23T20:00:00-07:00`) or an RFC 3339 string.
#[derive(Debug, Clone, Eq)]
pub struct SpecTime {
    /// Seconds since the Unix epoch.
    pub secs: i64,
    /// As written.
    pub text: String,
}

impl PartialEq for SpecTime {
    fn eq(&self, other: &Self) -> bool {
        self.secs == other.secs
    }
}

impl SpecTime {
    /// From RFC 3339 text with an offset.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        time::parse_rfc3339(text).map(|secs| SpecTime { secs, text: text.to_owned() }).ok_or_else(|| {
            format!(
                "`{text}` needs a date, a time and a UTC offset (e.g. 2026-10-23T20:00:00-07:00 or …Z); \
                 guildsmith never guesses a time zone"
            )
        })
    }

    /// A UTC time.
    #[must_use]
    pub fn utc(secs: i64) -> Self {
        SpecTime { secs, text: time::rfc3339(u64::try_from(secs).unwrap_or(0)) }
    }

    /// RFC 3339 in UTC, as Discord takes it.
    #[must_use]
    pub fn to_utc(&self) -> String {
        time::rfc3339(u64::try_from(self.secs).unwrap_or(0))
    }
}

impl fmt::Display for SpecTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for SpecTime {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for SpecTime {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = match toml::Value::deserialize(d)? {
            toml::Value::Datetime(dt) => {
                if dt.date.is_none() || dt.time.is_none() || dt.offset.is_none() {
                    return Err(serde::de::Error::custom(format!(
                        "`{dt}` has no UTC offset; write it like 2026-10-23T20:00:00-07:00 or …Z \
                         (guildsmith never guesses a time zone)"
                    )));
                }
                dt.to_string()
            }
            toml::Value::String(s) => s,
            other => return Err(serde::de::Error::custom(format!("expected a datetime, got {}", other.type_str()))),
        };
        SpecTime::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for SpecTime {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SpecTime".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "date-time",
            "description": "A date and time with a UTC offset, e.g. 2026-10-23T20:00:00-07:00."
        })
    }
}

/// Where an event happens: `{ voice = "Lobby" }`, `{ stage = "Town Hall" }` or
/// `{ external = { place = "The park" } }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LocationSpec {
    /// A voice channel (name or id).
    Voice(String),
    /// A stage channel (name or id).
    Stage(String),
    /// A place outside Discord; the event needs an `end`.
    External { place: String },
}

/// `[[events]]`: a scheduled event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    /// Written back by apply. Without it, the event is matched by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 1–100 characters.
    pub name: String,
    /// At most 1000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: SpecTime,
    /// Required for external events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<SpecTime>,
    pub location: LocationSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<Recurrence>,
    /// Cover image: {file}, {url} or {data}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Asset>,
}

impl EventSpec {
    /// Everything wrong with this event on its own.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !(1..=MAX_NAME).contains(&self.name.chars().count()) {
            out.push("event names are 1–100 characters".to_owned());
        }
        if self.description.as_ref().is_some_and(|d| d.chars().count() > MAX_DESCRIPTION) {
            out.push("event descriptions are at most 1000 characters".to_owned());
        }
        if let LocationSpec::External { place } = &self.location {
            if self.end.is_none() {
                out.push("an external event needs an `end`".to_owned());
            }
            if !(1..=100).contains(&place.chars().count()) {
                out.push("an external place is 1–100 characters".to_owned());
            }
        }
        if let Some(end) = &self.end
            && end.secs <= self.start.secs
        {
            out.push("`end` must be after `start`".to_owned());
        }
        if let Some(r) = &self.recurrence
            && let Err(e) = recurrence_rule(r, &self.start.to_utc())
        {
            out.push(e);
        }
        out
    }
}
