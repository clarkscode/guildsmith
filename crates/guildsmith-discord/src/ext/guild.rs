//! Guild settings as JSON (`GET`/`PATCH guilds/{g}`): twilight 0.17.1's update can't set
//! `description` or `safety_alerts_channel_id` (research E3).

use std::collections::BTreeSet;

use guildsmith_app::ports::{GuildSettings, PortResult, SettingsChange};
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::Client;
use twilight_http::request::Method;

use crate::raw;

#[derive(Debug, Deserialize)]
struct RawGuild {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    banner: Option<String>,
    #[serde(default)]
    splash: Option<String>,
    #[serde(default)]
    system_channel_id: Option<Id>,
    #[serde(default)]
    system_channel_flags: u64,
    #[serde(default)]
    rules_channel_id: Option<Id>,
    #[serde(default)]
    public_updates_channel_id: Option<Id>,
    #[serde(default)]
    safety_alerts_channel_id: Option<Id>,
    #[serde(default)]
    afk_channel_id: Option<Id>,
    #[serde(default)]
    afk_timeout: u32,
    #[serde(default)]
    verification_level: u8,
    #[serde(default)]
    default_message_notifications: u8,
    #[serde(default)]
    explicit_content_filter: u8,
    #[serde(default)]
    preferred_locale: String,
    #[serde(default)]
    premium_progress_bar_enabled: bool,
    #[serde(default)]
    features: BTreeSet<String>,
}

impl From<RawGuild> for GuildSettings {
    fn from(g: RawGuild) -> Self {
        GuildSettings {
            name: g.name,
            description: g.description,
            icon: g.icon,
            banner: g.banner,
            splash: g.splash,
            system_channel_id: g.system_channel_id,
            system_channel_flags: g.system_channel_flags,
            rules_channel_id: g.rules_channel_id,
            public_updates_channel_id: g.public_updates_channel_id,
            safety_alerts_channel_id: g.safety_alerts_channel_id,
            afk_channel_id: g.afk_channel_id,
            afk_timeout: g.afk_timeout,
            verification_level: g.verification_level,
            default_notifications: g.default_message_notifications,
            explicit_content_filter: g.explicit_content_filter,
            preferred_locale: g.preferred_locale,
            premium_progress_bar: g.premium_progress_bar_enabled,
            features: g.features,
        }
    }
}

/// The PATCH body: only the fields that change; `null` clears.
pub(crate) fn body(c: &SettingsChange) -> Value {
    let mut b = Map::new();
    let mut put = |k: &str, v: Value| {
        b.insert(k.to_owned(), v);
    };
    if let Some(v) = &c.name {
        put("name", json!(v));
    }
    if let Some(v) = &c.description {
        put("description", json!(v));
    }
    if let Some(v) = &c.icon {
        put("icon", json!(v));
    }
    if let Some(v) = &c.banner {
        put("banner", json!(v));
    }
    if let Some(v) = &c.splash {
        put("splash", json!(v));
    }
    if let Some(v) = &c.system_channel_id {
        put("system_channel_id", json!(v));
    }
    if let Some(v) = c.system_channel_flags {
        put("system_channel_flags", json!(v));
    }
    if let Some(v) = &c.rules_channel_id {
        put("rules_channel_id", json!(v));
    }
    if let Some(v) = &c.public_updates_channel_id {
        put("public_updates_channel_id", json!(v));
    }
    if let Some(v) = &c.safety_alerts_channel_id {
        put("safety_alerts_channel_id", json!(v));
    }
    if let Some(v) = &c.afk_channel_id {
        put("afk_channel_id", json!(v));
    }
    if let Some(v) = c.afk_timeout {
        put("afk_timeout", json!(v));
    }
    if let Some(v) = c.verification_level {
        put("verification_level", json!(v));
    }
    if let Some(v) = c.default_notifications {
        put("default_message_notifications", json!(v));
    }
    if let Some(v) = c.explicit_content_filter {
        put("explicit_content_filter", json!(v));
    }
    if let Some(v) = &c.preferred_locale {
        put("preferred_locale", json!(v));
    }
    if let Some(v) = c.premium_progress_bar {
        put("premium_progress_bar_enabled", json!(v));
    }
    if let Some(v) = &c.features {
        put("features", json!(v));
    }
    Value::Object(b)
}

pub(crate) async fn get(http: &Client, guild: Id) -> PortResult<GuildSettings> {
    let g: RawGuild = raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}"), &[], None, None).await?;
    Ok(g.into())
}

pub(crate) async fn update(http: &Client, guild: Id, c: &SettingsChange, reason: &str) -> PortResult<GuildSettings> {
    let res = raw::send(http, Method::Patch, &format!("guilds/{guild}"), &[], Some(&body(c)), Some(reason)).await?;
    Ok(raw::parse::<RawGuild>(&res, "update server settings")?.into())
}
