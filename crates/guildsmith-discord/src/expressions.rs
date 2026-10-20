//! `ExpressionsPort` for the real Discord API: emoji as JSON with data URIs, stickers as a
//! multipart upload, sounds through `ext::soundboard`.

use guildsmith_app::ports::{
    ExpressionEdit, ExpressionInfo, ExpressionKind, ExpressionsPort, NewExpression, PortResult,
};
use guildsmith_core::Id;
use guildsmith_core::asset::data_uri;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::request::{Form, Method};

use crate::port::TwilightDiscord;
use crate::{ext, raw};

#[derive(Debug, Deserialize)]
struct RawEmoji {
    id: Id,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    roles: Vec<Id>,
    #[serde(default)]
    animated: bool,
}

impl From<RawEmoji> for ExpressionInfo {
    fn from(e: RawEmoji) -> Self {
        let ext = if e.animated { "gif" } else { "png" };
        ExpressionInfo {
            kind: ExpressionKind::Emoji,
            id: e.id,
            name: e.name.unwrap_or_default(),
            animated: e.animated,
            roles: e.roles,
            description: None,
            tags: None,
            format: None,
            volume: None,
            emoji: None,
            url: format!("https://cdn.discordapp.com/emojis/{}.{ext}", e.id),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawSticker {
    id: Id,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: String,
    format_type: u8,
}

impl From<RawSticker> for ExpressionInfo {
    fn from(s: RawSticker) -> Self {
        let ext = match s.format_type {
            3 => "json",
            4 => "gif",
            _ => "png",
        };
        ExpressionInfo {
            kind: ExpressionKind::Sticker,
            id: s.id,
            name: s.name,
            animated: matches!(s.format_type, 2 | 4),
            roles: Vec::new(),
            description: s.description,
            tags: Some(s.tags),
            format: Some(s.format_type),
            volume: None,
            emoji: None,
            url: format!("https://media.discordapp.net/stickers/{}.{ext}", s.id),
        }
    }
}

impl ExpressionsPort for TwilightDiscord {
    async fn expressions(&self, guild: Id, kind: ExpressionKind) -> PortResult<Vec<ExpressionInfo>> {
        let http = self.http().await?;
        match kind {
            ExpressionKind::Emoji => {
                let l: Vec<RawEmoji> =
                    raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/emojis"), &[], None, None).await?;
                Ok(l.into_iter().map(ExpressionInfo::from).collect())
            }
            ExpressionKind::Sticker => {
                let l: Vec<RawSticker> =
                    raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/stickers"), &[], None, None).await?;
                Ok(l.into_iter().map(ExpressionInfo::from).collect())
            }
            ExpressionKind::Sound => ext::soundboard::list(http, guild).await,
        }
    }

    async fn create_expression(&self, guild: Id, new: &NewExpression, reason: &str) -> PortResult<ExpressionInfo> {
        let http = self.http().await?;
        match new.kind {
            ExpressionKind::Emoji => {
                let body = json!({"name": new.name, "image": data_uri(new.file_type, &new.bytes), "roles": new.roles});
                let res =
                    raw::send(http, Method::Post, &format!("guilds/{guild}/emojis"), &[], Some(&body), Some(reason))
                        .await?;
                Ok(raw::parse::<RawEmoji>(&res, "create emoji")?.into())
            }
            ExpressionKind::Sticker => {
                let file = format!("sticker.{}", new.file_type.ext());
                let form = Form::new()
                    .part(b"name", new.name.as_bytes())
                    .part(b"description", new.description.as_bytes())
                    .part(b"tags", new.tags.as_bytes())
                    .file_part(b"file", file.as_bytes(), &new.bytes);
                let res =
                    raw::send_form(http, Method::Post, &format!("guilds/{guild}/stickers"), form, Some(reason)).await?;
                Ok(raw::parse::<RawSticker>(&res, "create sticker")?.into())
            }
            ExpressionKind::Sound => ext::soundboard::create(http, guild, new, reason).await,
        }
    }

    async fn update_expression(
        &self,
        guild: Id,
        kind: ExpressionKind,
        id: Id,
        e: &ExpressionEdit,
        reason: &str,
    ) -> PortResult<ExpressionInfo> {
        let http = self.http().await?;
        let mut body = Map::new();
        if let Some(n) = &e.name {
            body.insert("name".to_owned(), json!(n));
        }
        match kind {
            ExpressionKind::Emoji => {
                if let Some(r) = &e.roles {
                    body.insert("roles".to_owned(), json!(r));
                }
                let path = format!("guilds/{guild}/emojis/{id}");
                let res = raw::send(http, Method::Patch, &path, &[], Some(&Value::Object(body)), Some(reason)).await?;
                Ok(raw::parse::<RawEmoji>(&res, "update emoji")?.into())
            }
            ExpressionKind::Sticker => {
                if let Some(d) = &e.description {
                    body.insert("description".to_owned(), json!(d));
                }
                if let Some(t) = &e.tags {
                    body.insert("tags".to_owned(), json!(t));
                }
                let path = format!("guilds/{guild}/stickers/{id}");
                let res = raw::send(http, Method::Patch, &path, &[], Some(&Value::Object(body)), Some(reason)).await?;
                Ok(raw::parse::<RawSticker>(&res, "update sticker")?.into())
            }
            ExpressionKind::Sound => ext::soundboard::update(http, guild, id, e, reason).await,
        }
    }

    async fn delete_expression(&self, guild: Id, kind: ExpressionKind, id: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        let path = match kind {
            ExpressionKind::Emoji => format!("guilds/{guild}/emojis/{id}"),
            ExpressionKind::Sticker => format!("guilds/{guild}/stickers/{id}"),
            ExpressionKind::Sound => return ext::soundboard::delete(http, guild, id, reason).await,
        };
        raw::send(http, Method::Delete, &path, &[], None, Some(reason)).await?;
        Ok(())
    }
}
