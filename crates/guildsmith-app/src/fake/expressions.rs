//! `ExpressionsPort` for the fake: emoji, stickers and sounds, with uploaded bytes served at
//! their CDN URLs (so plans can compare them, research E7).

use std::collections::BTreeMap;

use guildsmith_core::asset::FileType;
use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, next_id, not_found};
use crate::ports::{
    ExpressionEdit, ExpressionInfo, ExpressionKind, ExpressionsPort, NewExpression, PortError, PortResult,
};

/// The fake's expressions for one server.
#[derive(Debug, Clone, Default)]
pub struct ExpressionsState {
    pub items: BTreeMap<Id, ExpressionInfo>,
}

fn url(kind: ExpressionKind, id: Id, t: FileType) -> String {
    match kind {
        ExpressionKind::Emoji => format!("https://cdn.discordapp.com/emojis/{id}.{}", t.ext()),
        ExpressionKind::Sticker => format!("https://media.discordapp.net/stickers/{id}.{}", t.ext()),
        ExpressionKind::Sound => format!("https://cdn.discordapp.com/soundboard-sounds/{id}"),
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl ExpressionsPort for FakeDiscord {
    async fn expressions(&self, guild: Id, kind: ExpressionKind) -> PortResult<Vec<ExpressionInfo>> {
        self.read_guild("expressions", guild, None, |g| {
            Ok(g.res
                .peek::<ExpressionsState>()
                .map(|s| s.items.values().filter(|e| e.kind == kind).cloned().collect())
                .unwrap_or_default())
        })
    }

    async fn create_expression(&self, guild: Id, new: &NewExpression, reason: &str) -> PortResult<ExpressionInfo> {
        let mut served = None;
        let info = self.write("create_expression", guild, None, Some(reason), |g, next| {
            g.bot_can(Permission::CreateGuildExpressions)?;
            if new.kind == ExpressionKind::Sticker
                && new.file_type == FileType::Lottie
                && !g.snapshot.guild.features.iter().any(|f| f == "VERIFIED" || f == "PARTNERED")
            {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(50045),
                    message: "Lottie stickers need a verified or partnered server".to_owned(),
                });
            }
            let id = next_id(next);
            let info = ExpressionInfo {
                kind: new.kind,
                id,
                name: new.name.clone(),
                animated: new.file_type == FileType::Gif || new.file_type == FileType::Apng,
                roles: new.roles.clone(),
                description: (new.kind == ExpressionKind::Sticker).then(|| new.description.clone()),
                tags: (new.kind == ExpressionKind::Sticker).then(|| new.tags.clone()),
                format: (new.kind == ExpressionKind::Sticker).then_some(match new.file_type {
                    FileType::Apng => 2,
                    FileType::Lottie => 3,
                    FileType::Gif => 4,
                    _ => 1,
                }),
                volume: (new.kind == ExpressionKind::Sound).then(|| new.volume.unwrap_or(1.0)),
                emoji: new.emoji.clone(),
                url: url(new.kind, id, new.file_type),
            };
            served = Some((info.url.clone(), new.bytes.clone()));
            g.res.get::<ExpressionsState>().items.insert(id, info.clone());
            Ok(info)
        })?;
        if let Some((u, bytes)) = served {
            self.set_url(&u, bytes);
        }
        Ok(info)
    }

    async fn update_expression(
        &self,
        guild: Id,
        kind: ExpressionKind,
        id: Id,
        e: &ExpressionEdit,
        reason: &str,
    ) -> PortResult<ExpressionInfo> {
        self.write("update_expression", guild, Some(id), Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuildExpressions)?;
            let item = g
                .res
                .get::<ExpressionsState>()
                .items
                .get_mut(&id)
                .filter(|x| x.kind == kind)
                .ok_or_else(|| not_found(kind.name(), id))?;
            if let Some(v) = &e.name {
                item.name.clone_from(v);
            }
            if let Some(v) = &e.roles {
                item.roles.clone_from(v);
            }
            if let Some(v) = &e.description {
                item.description = Some(v.clone());
            }
            if let Some(v) = &e.tags {
                item.tags = Some(v.clone());
            }
            if let Some(v) = e.volume {
                item.volume = Some(v);
            }
            if let Some(v) = &e.emoji {
                item.emoji.clone_from(v);
            }
            Ok(item.clone())
        })
    }

    async fn delete_expression(&self, guild: Id, kind: ExpressionKind, id: Id, reason: &str) -> PortResult<()> {
        self.write("delete_expression", guild, Some(id), Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuildExpressions)?;
            let s = g.res.get::<ExpressionsState>();
            if s.items.get(&id).is_none_or(|x| x.kind != kind) {
                return Err(not_found(kind.name(), id));
            }
            s.items.remove(&id);
            Ok(())
        })
    }
}
