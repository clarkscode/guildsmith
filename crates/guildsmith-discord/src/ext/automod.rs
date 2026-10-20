//! Auto-moderation rule bodies, built here because twilight 0.17.1's update sends `null` for
//! unset fields and lacks `mention_raid_protection_enabled`, the member-profile trigger and the
//! block-member-interaction action (research E3). Unset fields are left out.

use guildsmith_app::ports::{PortError, PortResult};
use guildsmith_core::Id;
use guildsmith_core::spec::automod::{Action, AutomodRule, Keywords, Preset, Trigger};
use serde::Deserialize;
use serde_json::{Map, Value, json};

fn keywords_json(k: &Keywords) -> Value {
    let mut m = Map::new();
    if !k.keywords.is_empty() {
        m.insert("keyword_filter".to_owned(), json!(k.keywords));
    }
    if !k.regex.is_empty() {
        m.insert("regex_patterns".to_owned(), json!(k.regex));
    }
    if !k.allow.is_empty() {
        m.insert("allow_list".to_owned(), json!(k.allow));
    }
    Value::Object(m)
}

fn metadata(t: &Trigger) -> Option<Value> {
    match t {
        Trigger::Keyword(k) | Trigger::MemberProfile(k) => Some(keywords_json(k)),
        Trigger::Spam => None,
        Trigger::Preset { presets, allow } => {
            let mut m = json!({"presets": presets.iter().map(|p| p.code()).collect::<Vec<_>>()});
            if !allow.is_empty() {
                m["allow_list"] = json!(allow);
            }
            Some(m)
        }
        Trigger::MentionSpam { limit, raid_protection } => {
            Some(json!({"mention_total_limit": limit, "mention_raid_protection_enabled": raid_protection}))
        }
    }
}

fn action_json(a: &Action<Id>) -> Value {
    match a {
        Action::Block { message: Some(m) } => json!({"type": 1, "metadata": {"custom_message": m}}),
        Action::Block { message: None } => json!({"type": 1}),
        Action::Alert { channel } => json!({"type": 2, "metadata": {"channel_id": channel}}),
        Action::Timeout { duration } => json!({"type": 3, "metadata": {"duration_seconds": duration}}),
        Action::BlockInteraction => json!({"type": 4}),
    }
}

/// The body to create a rule (`create`), or to replace an existing rule's settings (no
/// `trigger_type`, which can't change).
pub(crate) fn body(r: &AutomodRule<Id>, create: bool) -> Value {
    let mut b = Map::new();
    b.insert("name".to_owned(), json!(r.name));
    b.insert("event_type".to_owned(), json!(r.trigger.event_type()));
    if create {
        b.insert("trigger_type".to_owned(), json!(r.trigger.code()));
    }
    if let Some(m) = metadata(&r.trigger) {
        b.insert("trigger_metadata".to_owned(), m);
    }
    b.insert("actions".to_owned(), Value::Array(r.actions.iter().map(action_json).collect()));
    b.insert("enabled".to_owned(), json!(r.enabled));
    b.insert("exempt_roles".to_owned(), json!(r.exempt_roles));
    b.insert("exempt_channels".to_owned(), json!(r.exempt_channels));
    Value::Object(b)
}

#[derive(Debug, Default, Deserialize)]
struct RawMeta {
    #[serde(default)]
    keyword_filter: Vec<String>,
    #[serde(default)]
    regex_patterns: Vec<String>,
    #[serde(default)]
    allow_list: Vec<String>,
    #[serde(default)]
    presets: Vec<u8>,
    #[serde(default)]
    mention_total_limit: Option<u8>,
    #[serde(default)]
    mention_raid_protection_enabled: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawActionMeta {
    #[serde(default)]
    channel_id: Option<Id>,
    #[serde(default)]
    duration_seconds: Option<u32>,
    #[serde(default)]
    custom_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAction {
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    metadata: Option<RawActionMeta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawRule {
    id: Id,
    name: String,
    trigger_type: u8,
    #[serde(default)]
    trigger_metadata: Option<RawMeta>,
    #[serde(default)]
    actions: Vec<RawAction>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    exempt_roles: Vec<Id>,
    #[serde(default)]
    exempt_channels: Vec<Id>,
}

fn unknown(what: &str) -> PortError {
    PortError::Discord {
        status: 0,
        code: None,
        message: format!("unknown automod {what}; guildsmith can't show this rule"),
    }
}

impl TryFrom<RawRule> for AutomodRule<Id> {
    type Error = PortError;

    fn try_from(r: RawRule) -> PortResult<Self> {
        let m = r.trigger_metadata.unwrap_or_default();
        let kw = || Keywords {
            keywords: m.keyword_filter.clone(),
            regex: m.regex_patterns.clone(),
            allow: m.allow_list.clone(),
        };
        let trigger = match r.trigger_type {
            1 => Trigger::Keyword(kw()),
            3 => Trigger::Spam,
            4 => Trigger::Preset {
                presets: m.presets.iter().filter_map(|p| Preset::from_code(*p)).collect(),
                allow: m.allow_list.clone(),
            },
            5 => Trigger::MentionSpam {
                limit: m.mention_total_limit.unwrap_or(0),
                raid_protection: m.mention_raid_protection_enabled,
            },
            6 => Trigger::MemberProfile(kw()),
            _ => return Err(unknown("trigger")),
        };
        let actions = r
            .actions
            .into_iter()
            .map(|a| {
                let meta = a.metadata.unwrap_or_default();
                Ok(match a.kind {
                    1 => Action::Block { message: meta.custom_message.filter(|m| !m.is_empty()) },
                    2 => Action::Alert { channel: meta.channel_id.ok_or_else(|| unknown("alert channel"))? },
                    3 => Action::Timeout { duration: meta.duration_seconds.unwrap_or(0) },
                    4 => Action::BlockInteraction,
                    _ => return Err(unknown("action")),
                })
            })
            .collect::<PortResult<Vec<_>>>()?;
        Ok(AutomodRule {
            id: Some(r.id),
            name: r.name,
            enabled: r.enabled,
            trigger,
            actions,
            exempt_roles: r.exempt_roles,
            exempt_channels: r.exempt_channels,
        })
    }
}
