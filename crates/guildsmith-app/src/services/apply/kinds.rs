//! Running ops of the kinds beyond roles and channels through each group's port. Files are
//! read again and must still have the hash the plan was made with.

use guildsmith_core::Id;
use guildsmith_core::asset::{AssetKind, data_uri};
use guildsmith_core::live::{
    EventLocation, ExpressionKind, Onboarding, OnboardingOption, OnboardingPrompt, WelcomeChannel, WelcomeScreen,
};
use guildsmith_core::planner::ops::{
    EventSet, ExpressionSet, FileOp, LocationSet, MessageBody, NewExpressionOp, OnboardingSet, SettingsSet,
};
use guildsmith_core::planner::{OpKind, Ref};

use super::{CreatedSecret, Exec};
use crate::assets;
use crate::date;
use crate::error::ServiceError;
use crate::ports::{
    AllowedMentions, DiscordPort, EventFields, ExpressionEdit, MessageEdit, NewExpression, NewInvite, OutgoingMessage,
    SettingsChange, WebhookFields,
};
use crate::secrets::webhook_url;
use crate::services::settings::new_id;

fn asset_kind(kind: ExpressionKind) -> AssetKind {
    match kind {
        ExpressionKind::Emoji => AssetKind::Emoji,
        ExpressionKind::Sticker => AssetKind::Sticker,
        ExpressionKind::Sound => AssetKind::Sound,
    }
}

impl<P: DiscordPort> Exec<'_, P> {
    fn ids(&self, list: &[Ref]) -> Result<Vec<Id>, ServiceError> {
        list.iter().map(|r| self.resolve(r).map_err(Into::into)).collect()
    }

    #[allow(clippy::option_option)] // three states: leave alone, clear, set
    fn opt(&self, r: Option<&Option<Ref>>) -> Result<Option<Option<Id>>, ServiceError> {
        Ok(match r {
            None => None,
            Some(None) => Some(None),
            Some(Some(r)) => Some(Some(self.resolve(r)?)),
        })
    }

    /// The file's bytes as a data URI, checked against the plan's hash.
    async fn file(&self, f: &FileOp, kind: AssetKind) -> Result<(Vec<u8>, String), ServiceError> {
        let l = assets::load(&self.app.port, &self.app.policy, self.caller, &f.asset, kind).await?;
        if l.hash != f.hash {
            return Err(ServiceError::Invalid(format!(
                "{} changed since the plan was made; plan and apply again",
                f.asset
            )));
        }
        let uri = data_uri(l.file_type, &l.bytes);
        Ok((l.bytes, uri))
    }

    async fn image(&self, f: Option<&Option<FileOp>>) -> Result<Option<Option<String>>, ServiceError> {
        Ok(match f {
            None => None,
            Some(None) => Some(None),
            Some(Some(f)) => Some(Some(self.file(f, AssetKind::GuildImage).await?.1)),
        })
    }

    async fn settings(&self, set: &SettingsSet, reason: &str) -> Result<(), ServiceError> {
        let change = SettingsChange {
            name: set.name.clone(),
            description: set.description.clone(),
            icon: self.image(set.icon.as_ref()).await?,
            banner: self.image(set.banner.as_ref()).await?,
            splash: self.image(set.splash.as_ref()).await?,
            system_channel_id: self.opt(set.system_channel.as_ref())?,
            system_channel_flags: set.system_channel_flags,
            rules_channel_id: self.opt(set.rules_channel.as_ref())?,
            public_updates_channel_id: self.opt(set.public_updates_channel.as_ref())?,
            safety_alerts_channel_id: self.opt(set.safety_alerts_channel.as_ref())?,
            afk_channel_id: self.opt(set.afk_channel.as_ref())?,
            afk_timeout: set.afk_timeout,
            verification_level: set.verification_level,
            default_notifications: set.default_notifications,
            explicit_content_filter: set.explicit_content_filter,
            preferred_locale: set.preferred_locale.clone(),
            premium_progress_bar: set.premium_progress_bar,
            features: set.features.clone(),
        };
        self.app.port.update_guild_settings(self.guild, &change, reason).await?;
        Ok(())
    }

    fn onboarding(&self, o: &OnboardingSet) -> Result<Onboarding, ServiceError> {
        let now_ms = date::now_secs(self.app.clock.as_ref()) * 1000;
        let mut n = 0;
        let mut prompts = Vec::new();
        for p in &o.prompts {
            let mut options = Vec::new();
            for opt in &p.options {
                options.push(OnboardingOption {
                    id: opt.id.unwrap_or_else(|| new_id(now_ms, &mut n)),
                    title: opt.title.clone(),
                    description: opt.description.clone(),
                    emoji: opt.emoji.clone(),
                    role_ids: self.ids(&opt.roles)?,
                    channel_ids: self.ids(&opt.channels)?,
                });
            }
            prompts.push(OnboardingPrompt {
                id: p.id.unwrap_or_else(|| new_id(now_ms, &mut n)),
                title: p.title.clone(),
                kind: p.kind,
                single_select: p.single_select,
                required: p.required,
                in_onboarding: p.in_onboarding,
                options,
            });
        }
        Ok(Onboarding {
            enabled: o.enabled,
            mode: o.mode,
            default_channel_ids: self.ids(&o.default_channels)?,
            prompts,
        })
    }

    async fn create_expression(
        &mut self,
        kind: ExpressionKind,
        key: &str,
        new: &NewExpressionOp,
        reason: &str,
    ) -> Result<(), ServiceError> {
        let l = assets::load(&self.app.port, &self.app.policy, self.caller, &new.file.asset, asset_kind(kind)).await?;
        if l.hash != new.file.hash {
            return Err(ServiceError::Invalid(format!(
                "{} changed since the plan was made; plan and apply again",
                new.file.asset
            )));
        }
        let info = self
            .app
            .port
            .create_expression(
                self.guild,
                &NewExpression {
                    kind,
                    name: new.name.clone(),
                    bytes: l.bytes,
                    file_type: l.file_type,
                    roles: self.ids(&new.roles)?,
                    description: new.description.clone(),
                    tags: new.tags.clone(),
                    volume: new.volume.map(|v| v.0),
                    emoji: new.emoji.clone(),
                },
                reason,
            )
            .await?;
        self.created.insert(key.to_owned(), info.id);
        Ok(())
    }

    fn expression_edit(&self, set: &ExpressionSet) -> Result<ExpressionEdit, ServiceError> {
        Ok(ExpressionEdit {
            name: set.name.clone(),
            roles: set.roles.as_deref().map(|r| self.ids(r)).transpose()?,
            description: set.description.clone(),
            tags: set.tags.clone(),
            volume: set.volume.map(|v| v.0),
            emoji: set.emoji.clone(),
        })
    }

    fn location(&self, l: &LocationSet) -> Result<EventLocation, ServiceError> {
        Ok(match l {
            LocationSet::Voice(r) => EventLocation::Voice(self.resolve(r)?),
            LocationSet::Stage(r) => EventLocation::Stage(self.resolve(r)?),
            LocationSet::External(p) => EventLocation::External(p.clone()),
        })
    }

    async fn event_fields(&self, e: &EventSet) -> Result<EventFields, ServiceError> {
        Ok(EventFields {
            name: e.name.clone(),
            description: e.description.clone(),
            start: e.start.clone(),
            end: e.end.clone(),
            location: e.location.as_ref().map(|l| self.location(l)).transpose()?,
            status: None,
            recurrence: e.recurrence.clone(),
            image: match &e.image {
                Some(f) => Some(self.file(f, AssetKind::GuildImage).await?.1),
                None => None,
            },
        })
    }

    async fn webhook(
        &mut self,
        key: &str,
        name: &str,
        channel: &Ref,
        avatar: Option<&FileOp>,
        reason: &str,
    ) -> Result<(), ServiceError> {
        let avatar = match avatar {
            Some(f) => Some(self.file(f, AssetKind::Avatar).await?.1),
            None => None,
        };
        let cid = self.resolve(channel)?;
        let (info, token) = self.app.port.create_webhook(cid, name, avatar.as_deref(), reason).await?;
        self.secrets.push(CreatedSecret { webhook: info.name.clone(), url: webhook_url(info.id, &token) });
        self.app.webhook_tokens.insert(info.id, token);
        self.created.insert(key.to_owned(), info.id);
        Ok(())
    }

    async fn post(
        &mut self,
        key: &str,
        channel: &Ref,
        body: &MessageBody,
        pin: bool,
        reason: &str,
    ) -> Result<(), ServiceError> {
        let cid = self.resolve(channel)?;
        let msg = OutgoingMessage {
            content: body.content.clone(),
            embeds: body.embeds.clone(),
            components: body.components.clone(),
            flags: body.flags,
            allowed_mentions: AllowedMentions::default(),
            ..OutgoingMessage::default()
        };
        let info = self.app.port.post(cid, &msg).await?;
        self.created.insert(key.to_owned(), info.id);
        if pin {
            self.app.port.set_pinned(cid, info.id, true, reason).await?;
        }
        Ok(())
    }

    /// Runs one op of the kinds beyond roles and channels.
    #[allow(clippy::too_many_lines)] // one arm per op
    pub(super) async fn kind(&mut self, kind: &OpKind, reason: &str) -> Result<(), ServiceError> {
        let (port, guild) = (&self.app.port, self.guild);
        match kind {
            OpKind::UpdateSettings { set, .. } => self.settings(set, reason).await?,
            OpKind::CreateAutomod { key, rule } => {
                let rule = rule.clone().map_refs(|r| self.resolve(&r))?;
                let made = port.create_automod(guild, &rule, reason).await?;
                if let Some(id) = made.id {
                    self.created.insert(key.clone(), id);
                }
            }
            OpKind::UpdateAutomod { id, rule, .. } => {
                let rule = rule.clone().map_refs(|r| self.resolve(&r))?;
                port.update_automod(guild, *id, &rule, reason).await?;
            }
            OpKind::DeleteAutomod { id } => port.delete_automod(guild, *id, reason).await?,
            OpKind::SetOnboarding { onboarding } => {
                let o = self.onboarding(onboarding)?;
                port.set_onboarding(guild, &o, reason).await?;
            }
            OpKind::SetWelcomeScreen { screen } => {
                let channels = screen
                    .channels
                    .iter()
                    .map(|c| {
                        Ok(WelcomeChannel {
                            channel_id: self.resolve(&c.channel)?,
                            description: c.description.clone(),
                            emoji: c.emoji.clone(),
                        })
                    })
                    .collect::<Result<_, ServiceError>>()?;
                let w = WelcomeScreen { enabled: screen.enabled, description: screen.description.clone(), channels };
                port.set_welcome_screen(guild, &w, reason).await?;
            }
            OpKind::CreateExpression { kind, key, new } => self.create_expression(*kind, key, new, reason).await?,
            OpKind::UpdateExpression { kind, id, set, .. } => {
                let edit = self.expression_edit(set)?;
                port.update_expression(guild, *kind, *id, &edit, reason).await?;
            }
            OpKind::DeleteExpression { kind, id } => port.delete_expression(guild, *kind, *id, reason).await?,
            OpKind::CreateWebhook { key, name, channel, avatar } => {
                self.webhook(key, name, channel, avatar.as_ref(), reason).await?;
            }
            OpKind::UpdateWebhook { id, name, channel, avatar, .. } => {
                let avatar = match avatar {
                    Some(f) => Some(Some(self.file(f, AssetKind::Avatar).await?.1)),
                    None => None,
                };
                let fields = WebhookFields {
                    name: name.clone(),
                    avatar,
                    channel_id: channel.as_ref().map(|c| self.resolve(c)).transpose()?,
                };
                port.update_webhook(*id, &fields, reason).await?;
            }
            OpKind::DeleteWebhook { id } => port.delete_webhook(*id, reason).await?,
            OpKind::CreateInvite { key, channel, max_age, max_uses, temporary, roles } => {
                let cid = self.resolve(channel)?;
                let new = NewInvite {
                    max_age: *max_age,
                    max_uses: *max_uses,
                    temporary: *temporary,
                    unique: true,
                    roles: self.ids(roles)?,
                };
                let info = port.create_invite(cid, &new, reason).await?;
                self.codes.insert(key.clone(), info.code);
            }
            OpKind::DeleteInvite { code } => port.delete_invite(code, reason).await?,
            OpKind::CreateEvent { key, set } => {
                let fields = self.event_fields(set).await?;
                let e = port.create_event(guild, &fields, reason).await?;
                self.created.insert(key.clone(), e.id);
            }
            OpKind::UpdateEvent { id, set, .. } => {
                let fields = self.event_fields(set).await?;
                port.update_event(guild, *id, &fields, reason).await?;
            }
            OpKind::DeleteEvent { id } => port.delete_event(guild, *id, reason).await?,
            OpKind::PostMessage { key, channel, body, pin } => self.post(key, channel, body, *pin, reason).await?,
            OpKind::EditMessage { channel, id, body, .. } => {
                let edit = MessageEdit {
                    content: Some(body.content.clone().unwrap_or_default()),
                    embeds: Some(body.embeds.clone()),
                    flags: Some(body.flags),
                    components: body.components.clone(),
                };
                port.edit(*channel, *id, &edit).await?;
            }
            OpKind::SetPinned { channel, id, pinned } => port.set_pinned(*channel, *id, *pinned, reason).await?,
            OpKind::DeleteMessage { channel, id } => port.delete_message(*channel, *id, reason).await?,
            other => {
                return Err(ServiceError::Invalid(format!("op {other:?} isn't one of the extra kinds")));
            }
        }
        Ok(())
    }
}
