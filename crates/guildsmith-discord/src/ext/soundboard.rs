//! Soundboard sounds (`guilds/{g}/soundboard-sounds[/{id}]`), missing in twilight 0.17.1, with
//! guildsmith's own model. The list comes as `{items: [...]}`; sounds are uploaded as MP3 or
//! Ogg data URIs.

use guildsmith_app::ports::{ExpressionEdit, ExpressionInfo, ExpressionKind, NewExpression, PortResult};
use guildsmith_core::Id;
use guildsmith_core::asset::data_uri;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::Client;
use twilight_http::request::Method;

use crate::raw;

#[derive(Debug, Deserialize)]
pub(crate) struct SoundboardSound {
    sound_id: Id,
    name: String,
    #[serde(default)]
    volume: f64,
    #[serde(default)]
    emoji_id: Option<Id>,
    #[serde(default)]
    emoji_name: Option<String>,
}

impl From<SoundboardSound> for ExpressionInfo {
    fn from(s: SoundboardSound) -> Self {
        ExpressionInfo {
            kind: ExpressionKind::Sound,
            id: s.sound_id,
            name: s.name,
            animated: false,
            roles: Vec::new(),
            description: None,
            tags: None,
            format: None,
            volume: Some(s.volume),
            emoji: s.emoji_id.map(|i| i.to_string()).or(s.emoji_name),
            url: format!("https://cdn.discordapp.com/soundboard-sounds/{}", s.sound_id),
        }
    }
}

#[derive(Debug, Deserialize)]
struct List {
    items: Vec<SoundboardSound>,
}

fn emoji_fields(map: &mut Map<String, Value>, emoji: Option<&str>) {
    match emoji {
        Some(e) if e.parse::<u64>().is_ok() => {
            map.insert("emoji_id".to_owned(), json!(e));
            map.insert("emoji_name".to_owned(), Value::Null);
        }
        Some(e) => {
            map.insert("emoji_id".to_owned(), Value::Null);
            map.insert("emoji_name".to_owned(), json!(e));
        }
        None => {
            map.insert("emoji_id".to_owned(), Value::Null);
            map.insert("emoji_name".to_owned(), Value::Null);
        }
    }
}

pub(crate) async fn list(http: &Client, guild: Id) -> PortResult<Vec<ExpressionInfo>> {
    let l: List =
        raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/soundboard-sounds"), &[], None, None).await?;
    Ok(l.items.into_iter().map(ExpressionInfo::from).collect())
}

pub(crate) async fn create(http: &Client, guild: Id, new: &NewExpression, reason: &str) -> PortResult<ExpressionInfo> {
    let mut body = Map::new();
    body.insert("name".to_owned(), json!(new.name));
    body.insert("sound".to_owned(), json!(data_uri(new.file_type, &new.bytes)));
    if let Some(v) = new.volume {
        body.insert("volume".to_owned(), json!(v));
    }
    if new.emoji.is_some() {
        emoji_fields(&mut body, new.emoji.as_deref());
    }
    let path = format!("guilds/{guild}/soundboard-sounds");
    let res = raw::send(http, Method::Post, &path, &[], Some(&Value::Object(body)), Some(reason)).await?;
    Ok(raw::parse::<SoundboardSound>(&res, "create sound")?.into())
}

pub(crate) async fn update(
    http: &Client,
    guild: Id,
    id: Id,
    e: &ExpressionEdit,
    reason: &str,
) -> PortResult<ExpressionInfo> {
    let mut body = Map::new();
    if let Some(n) = &e.name {
        body.insert("name".to_owned(), json!(n));
    }
    if let Some(v) = e.volume {
        body.insert("volume".to_owned(), json!(v));
    }
    if let Some(em) = &e.emoji {
        emoji_fields(&mut body, em.as_deref());
    }
    let path = format!("guilds/{guild}/soundboard-sounds/{id}");
    let res = raw::send(http, Method::Patch, &path, &[], Some(&Value::Object(body)), Some(reason)).await?;
    Ok(raw::parse::<SoundboardSound>(&res, "update sound")?.into())
}

pub(crate) async fn delete(http: &Client, guild: Id, id: Id, reason: &str) -> PortResult<()> {
    raw::send(http, Method::Delete, &format!("guilds/{guild}/soundboard-sounds/{id}"), &[], None, Some(reason)).await?;
    Ok(())
}
