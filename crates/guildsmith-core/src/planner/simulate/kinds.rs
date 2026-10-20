//! The simulator for kinds beyond roles and channels, following the same Discord rules as
//! `FakeDiscord`: new items get fresh ids, uploaded files are stored by their hash.

use super::Sim;
use crate::id::Id;
use crate::live::{
    EventInfo, EventLocation, ExpressionInfo, ExpressionKind, GuildSettings, InviteInfo, LiveMessage, Onboarding,
    OnboardingOption, OnboardingPrompt, WebhookInfo, WebhookKind, WelcomeChannel, WelcomeScreen, asset_key,
};
use crate::planner::ops::{EventSet, ExpressionSet, FileOp, LocationSet, OpKind, Ref, SettingsSet};

/// The hash Discord would show for an uploaded file.
fn cdn(f: &FileOp) -> String {
    f.hash.chars().take(32).collect()
}

impl Sim {
    fn refs(&self, list: &[Ref]) -> Result<Vec<Id>, String> {
        list.iter().map(|r| self.resolve(r)).collect()
    }

    fn opt_ref(&self, r: Option<&Option<Ref>>, old: Option<Id>) -> Result<Option<Id>, String> {
        match r {
            None => Ok(old),
            Some(None) => Ok(None),
            Some(Some(r)) => self.resolve(r).map(Some),
        }
    }

    fn image(&mut self, field: &str, f: Option<&Option<FileOp>>, old: &mut Option<String>) {
        match f {
            None => {}
            Some(None) => {
                *old = None;
                self.x.asset_hashes.remove(&asset_key("settings", field));
            }
            Some(Some(f)) => {
                *old = Some(cdn(f));
                self.x.asset_hashes.insert(asset_key("settings", field), f.hash.clone());
            }
        }
    }

    fn settings(&mut self, set: &SettingsSet) -> Result<(), String> {
        let mut g: GuildSettings = self.x.settings.clone().ok_or("settings were not read")?;
        if let Some(v) = &set.name {
            g.name.clone_from(v);
        }
        if let Some(v) = &set.description {
            g.description.clone_from(v);
        }
        self.image("icon", set.icon.as_ref(), &mut g.icon);
        self.image("banner", set.banner.as_ref(), &mut g.banner);
        self.image("splash", set.splash.as_ref(), &mut g.splash);
        g.system_channel_id = self.opt_ref(set.system_channel.as_ref(), g.system_channel_id)?;
        g.rules_channel_id = self.opt_ref(set.rules_channel.as_ref(), g.rules_channel_id)?;
        g.public_updates_channel_id = self.opt_ref(set.public_updates_channel.as_ref(), g.public_updates_channel_id)?;
        g.safety_alerts_channel_id = self.opt_ref(set.safety_alerts_channel.as_ref(), g.safety_alerts_channel_id)?;
        g.afk_channel_id = self.opt_ref(set.afk_channel.as_ref(), g.afk_channel_id)?;
        g.system_channel_flags = set.system_channel_flags.unwrap_or(g.system_channel_flags);
        g.afk_timeout = set.afk_timeout.unwrap_or(g.afk_timeout);
        g.verification_level = set.verification_level.unwrap_or(g.verification_level);
        g.default_notifications = set.default_notifications.unwrap_or(g.default_notifications);
        g.explicit_content_filter = set.explicit_content_filter.unwrap_or(g.explicit_content_filter);
        if let Some(v) = &set.preferred_locale {
            g.preferred_locale.clone_from(v);
        }
        g.premium_progress_bar = set.premium_progress_bar.unwrap_or(g.premium_progress_bar);
        if let Some(f) = &set.features {
            g.features.clone_from(f);
            self.s.guild.features.clone_from(f);
        }
        self.x.settings = Some(g);
        Ok(())
    }

    fn expression_edit(&self, e: &mut ExpressionInfo, set: &ExpressionSet) -> Result<(), String> {
        if let Some(v) = &set.name {
            e.name.clone_from(v);
        }
        if let Some(r) = &set.roles {
            e.roles = self.refs(r)?;
        }
        if let Some(v) = &set.description {
            e.description = Some(v.clone());
        }
        if let Some(v) = &set.tags {
            e.tags = Some(v.clone());
        }
        if let Some(v) = set.volume {
            e.volume = Some(v.0);
        }
        if let Some(v) = &set.emoji {
            e.emoji.clone_from(v);
        }
        Ok(())
    }

    fn location(&self, l: &LocationSet) -> Result<EventLocation, String> {
        Ok(match l {
            LocationSet::Voice(r) => EventLocation::Voice(self.resolve(r)?),
            LocationSet::Stage(r) => EventLocation::Stage(self.resolve(r)?),
            LocationSet::External(p) => EventLocation::External(p.clone()),
        })
    }

    fn event_edit(&mut self, e: &mut EventInfo, set: &EventSet) -> Result<(), String> {
        if let Some(v) = &set.name {
            e.name.clone_from(v);
        }
        if let Some(v) = &set.description {
            e.description.clone_from(v);
        }
        if let Some(v) = &set.start {
            e.start.clone_from(v);
        }
        if let Some(v) = &set.end {
            e.end = Some(v.clone());
        }
        if let Some(l) = &set.location {
            e.location = self.location(l)?;
        }
        if let Some(r) = &set.recurrence {
            e.recurrence = Some(r.clone());
        }
        if let Some(f) = &set.image {
            e.image = Some(cdn(f));
            self.x.asset_hashes.insert(asset_key("event", e.id), f.hash.clone());
        }
        Ok(())
    }

    fn onboarding(&mut self, o: &crate::planner::ops::OnboardingSet) -> Result<(), String> {
        let mut prompts = Vec::new();
        for p in &o.prompts {
            let mut options = Vec::new();
            for opt in &p.options {
                options.push(OnboardingOption {
                    id: opt.id.unwrap_or_else(|| self.fresh_id()),
                    title: opt.title.clone(),
                    description: opt.description.clone(),
                    emoji: opt.emoji.clone(),
                    role_ids: self.refs(&opt.roles)?,
                    channel_ids: self.refs(&opt.channels)?,
                });
            }
            prompts.push(OnboardingPrompt {
                id: p.id.unwrap_or_else(|| self.fresh_id()),
                title: p.title.clone(),
                kind: p.kind,
                single_select: p.single_select,
                required: p.required,
                in_onboarding: p.in_onboarding,
                options,
            });
        }
        self.x.onboarding = Some(Onboarding {
            enabled: o.enabled,
            mode: o.mode,
            default_channel_ids: self.refs(&o.default_channels)?,
            prompts,
        });
        Ok(())
    }

    /// Runs one op of the kinds beyond roles and channels.
    #[allow(clippy::too_many_lines)] // one arm per op
    pub(super) fn run_kind(&mut self, kind: &OpKind) -> Result<(), String> {
        let bot = self.s.bot.user_id;
        match kind {
            OpKind::UpdateSettings { set, .. } => self.settings(set)?,
            OpKind::CreateAutomod { key, rule } => {
                let id = self.fresh_id();
                let mut r = rule.clone().map_refs(|r| self.resolve(&r))?;
                r.id = Some(id);
                self.x.automod.get_or_insert_with(Vec::new).push(r);
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateAutomod { id, rule, .. } => {
                let mut r = rule.clone().map_refs(|r| self.resolve(&r))?;
                r.id = Some(*id);
                let list = self.x.automod.get_or_insert_with(Vec::new);
                let slot = list.iter_mut().find(|l| l.id == Some(*id)).ok_or("automod rule not found")?;
                *slot = r;
            }
            OpKind::DeleteAutomod { id } => self.x.automod.get_or_insert_with(Vec::new).retain(|l| l.id != Some(*id)),
            OpKind::SetOnboarding { onboarding } => self.onboarding(onboarding)?,
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
                    .collect::<Result<_, String>>()?;
                self.x.welcome_screen =
                    Some(WelcomeScreen { enabled: screen.enabled, description: screen.description.clone(), channels });
            }
            OpKind::CreateExpression { kind, key, new } => {
                let id = self.fresh_id();
                let e = ExpressionInfo {
                    kind: *kind,
                    id,
                    name: new.name.clone(),
                    animated: false,
                    roles: self.refs(&new.roles)?,
                    description: (*kind == ExpressionKind::Sticker).then(|| new.description.clone()),
                    tags: (*kind == ExpressionKind::Sticker).then(|| new.tags.clone()),
                    format: None,
                    volume: (*kind == ExpressionKind::Sound).then(|| new.volume.map_or(1.0, |v| v.0)),
                    emoji: new.emoji.clone(),
                    url: String::new(),
                };
                self.x.expressions.get_or_insert_with(Vec::new).push(e);
                self.x.asset_hashes.insert(asset_key(kind.name(), id), new.file.hash.clone());
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateExpression { id, set, .. } => {
                let mut list = self.x.expressions.take().unwrap_or_default();
                let res = match list.iter_mut().find(|e| e.id == *id) {
                    Some(e) => self.expression_edit(e, set),
                    None => Err("expression not found".to_owned()),
                };
                self.x.expressions = Some(list);
                res?;
            }
            OpKind::DeleteExpression { id, .. } => {
                self.x.expressions.get_or_insert_with(Vec::new).retain(|e| e.id != *id);
            }
            OpKind::CreateWebhook { key, name, channel, avatar } => {
                let id = self.fresh_id();
                let w = WebhookInfo {
                    id,
                    kind: WebhookKind::Incoming,
                    name: name.clone(),
                    channel_id: self.resolve(channel)?,
                    creator_id: Some(bot),
                    creator_name: None,
                    source: None,
                    avatar: avatar.as_ref().map(cdn),
                };
                if let Some(a) = avatar {
                    self.x.asset_hashes.insert(asset_key("webhook", id), a.hash.clone());
                }
                self.x.webhooks.get_or_insert_with(Vec::new).push(w);
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateWebhook { id, name, channel, avatar, .. } => {
                let channel = channel.as_ref().map(|c| self.resolve(c)).transpose()?;
                let list = self.x.webhooks.get_or_insert_with(Vec::new);
                let w = list.iter_mut().find(|w| w.id == *id).ok_or("webhook not found")?;
                if let Some(n) = name {
                    w.name.clone_from(n);
                }
                if let Some(c) = channel {
                    w.channel_id = c;
                }
                if let Some(a) = avatar {
                    w.avatar = Some(cdn(a));
                    self.x.asset_hashes.insert(asset_key("webhook", *id), a.hash.clone());
                }
            }
            OpKind::DeleteWebhook { id } => self.x.webhooks.get_or_insert_with(Vec::new).retain(|w| w.id != *id),
            OpKind::CreateInvite { key, channel, max_age, max_uses, temporary, roles } => {
                let code = format!("sim{}", self.fresh_id());
                let i = InviteInfo {
                    code: code.clone(),
                    channel_id: self.resolve(channel)?,
                    inviter_id: Some(bot),
                    inviter: None,
                    max_age: *max_age,
                    max_uses: *max_uses,
                    uses: 0,
                    temporary: *temporary,
                    created_at: String::new(),
                    expires_at: None,
                    roles: self.refs(roles)?,
                };
                self.x.invites.get_or_insert_with(Vec::new).push(i);
                self.codes.insert(key.clone(), code);
            }
            OpKind::DeleteInvite { code } => self.x.invites.get_or_insert_with(Vec::new).retain(|i| i.code != *code),
            OpKind::CreateEvent { key, set } => {
                let id = self.fresh_id();
                let mut e = EventInfo {
                    id,
                    name: String::new(),
                    description: None,
                    start: String::new(),
                    end: None,
                    status: "scheduled".to_owned(),
                    location: EventLocation::External(String::new()),
                    interested: 0,
                    creator: Some(bot),
                    recurrence: None,
                    image: None,
                };
                self.event_edit(&mut e, set)?;
                self.x.events.get_or_insert_with(Vec::new).push(e);
                self.created.insert(key.clone(), id);
            }
            OpKind::UpdateEvent { id, set, .. } => {
                let mut list = self.x.events.take().unwrap_or_default();
                let res = match list.iter_mut().find(|e| e.id == *id) {
                    Some(e) => self.event_edit(e, set),
                    None => Err("event not found".to_owned()),
                };
                self.x.events = Some(list);
                res?;
            }
            OpKind::DeleteEvent { id } => self.x.events.get_or_insert_with(Vec::new).retain(|e| e.id != *id),
            OpKind::PostMessage { key, channel, body, pin } => {
                let id = self.fresh_id();
                let channel = self.resolve(channel)?;
                let m = LiveMessage {
                    id,
                    content: body.content.clone().unwrap_or_default(),
                    embeds: body.embeds.clone(),
                    components: body.components.clone(),
                    pinned: *pin,
                    flags: body.flags,
                };
                self.x.messages.get_or_insert_with(Default::default).entry(channel).or_default().push(m);
                self.created.insert(key.clone(), id);
            }
            OpKind::EditMessage { channel, id, body, .. } => {
                let m = self.message(*channel, *id)?;
                m.content = body.content.clone().unwrap_or_default();
                m.embeds.clone_from(&body.embeds);
                m.components.clone_from(&body.components);
                m.flags = body.flags;
            }
            OpKind::SetPinned { channel, id, pinned } => self.message(*channel, *id)?.pinned = *pinned,
            OpKind::DeleteMessage { channel, id } => {
                if let Some(list) = self.x.messages.as_mut().and_then(|m| m.get_mut(channel)) {
                    list.retain(|m| m.id != *id);
                }
            }
            _ => return Err("not a kind op".to_owned()),
        }
        Ok(())
    }

    fn message(&mut self, channel: Id, id: Id) -> Result<&mut LiveMessage, String> {
        self.x
            .messages
            .as_mut()
            .and_then(|m| m.get_mut(&channel))
            .and_then(|l| l.iter_mut().find(|m| m.id == id))
            .ok_or_else(|| "message not found".to_owned())
    }
}
