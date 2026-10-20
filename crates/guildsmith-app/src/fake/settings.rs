//! `SettingsPort` for the fake: settings (seeded from the snapshot), onboarding (enabled only
//! in community servers), the welcome screen, one template per server, public templates set
//! by tests, integrations, the widget and the vanity URL.

use std::collections::BTreeMap;

use guildsmith_core::{Id, Permission};
use serde_json::Value;

use super::assets::uploaded;
use super::{FakeDiscord, FakeGuild, next_id};
use crate::ports::{
    GuildSettings, IntegrationInfo, Onboarding, PortError, PortResult, SettingsChange, SettingsPort, TemplateInfo,
    Vanity, WelcomeScreen, WidgetSettings,
};

/// Features a server can turn on or off itself.
pub const SETTABLE: [&str; 4] = ["COMMUNITY", "INVITES_DISABLED", "RAID_ALERTS_DISABLED", "DISCOVERABLE"];

/// The fake's settings state for one server.
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub settings: Option<GuildSettings>,
    pub onboarding: Onboarding,
    pub welcome: WelcomeScreen,
    pub templates: Vec<TemplateInfo>,
    pub integrations: Vec<IntegrationInfo>,
    pub widget: WidgetSettings,
    pub vanity: Option<String>,
}

fn bad(code: u64, message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(code), message: message.to_owned() }
}

impl FakeGuild {
    fn settings_now(&mut self) -> GuildSettings {
        let (name, features) = (self.snapshot.guild.name.clone(), self.snapshot.guild.features.clone());
        self.res
            .get::<SettingsState>()
            .settings
            .get_or_insert_with(|| GuildSettings {
                name,
                features,
                afk_timeout: 300,
                preferred_locale: "en-US".to_owned(),
                ..GuildSettings::default()
            })
            .clone()
    }
}

fn template(code: &str, name: &str, description: Option<&str>, source: Value) -> TemplateInfo {
    TemplateInfo {
        code: code.to_owned(),
        name: name.to_owned(),
        description: description.map(str::to_owned),
        usage_count: 0,
        created_at: "2026-10-19T12:00:00Z".to_owned(),
        updated_at: "2026-10-19T12:00:00Z".to_owned(),
        dirty: false,
        serialized_source_guild: source,
    }
}

/// Public templates, readable by code from any fake (Discord's `GET guilds/templates/{code}`).
#[derive(Debug, Clone, Default)]
pub(crate) struct PublicTemplates(pub BTreeMap<String, TemplateInfo>);

impl FakeDiscord {
    /// Makes a public template readable by code.
    pub fn add_public_template(&self, code: &str, name: &str, serialized_source_guild: Value) {
        self.lock().public_templates.0.insert(code.to_owned(), template(code, name, None, serialized_source_guild));
    }

    /// Adds an integration, as if it was set up in Discord.
    pub fn add_integration(&self, guild: Id, id: Id, name: &str, kind: &str) {
        self.edit_res::<SettingsState>(guild, |s| {
            s.integrations.push(IntegrationInfo {
                id,
                name: name.to_owned(),
                kind: kind.to_owned(),
                enabled: true,
                account: None,
            });
        });
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl SettingsPort for FakeDiscord {
    async fn guild_settings(&self, guild: Id) -> PortResult<GuildSettings> {
        let mut st = self.lock();
        let g = st.guilds.get_mut(&guild).ok_or(PortError::NotInGuild(guild))?;
        Ok(g.settings_now())
    }

    async fn update_guild_settings(&self, guild: Id, c: &SettingsChange, reason: &str) -> PortResult<GuildSettings> {
        // Uploaded images, as the CDN will serve them.
        let upload = |v: &Option<Option<String>>| v.clone().flatten().as_deref().and_then(uploaded);
        let images = [("icons", upload(&c.icon)), ("banners", upload(&c.banner)), ("splashes", upload(&c.splash))];
        let hash = |i: usize| images[i].1.as_ref().map(|(h, _)| h.clone());
        let updated = self.write("update_guild_settings", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            let mut s = g.settings_now();
            if let Some(f) = &c.features {
                let toggled: Vec<&str> =
                    SETTABLE.iter().copied().filter(|x| f.contains(*x) != s.features.contains(*x)).collect();
                if toggled.iter().any(|x| *x == "COMMUNITY" || *x == "DISCOVERABLE") {
                    g.bot_can(Permission::Administrator)?;
                }
                if f.contains("COMMUNITY")
                    && !s.features.contains("COMMUNITY")
                    && (c.rules_channel_id.unwrap_or(s.rules_channel_id).is_none()
                        || c.public_updates_channel_id.unwrap_or(s.public_updates_channel_id).is_none())
                {
                    return Err(bad(50035, "community servers need a rules channel and a public updates channel"));
                }
                let kept: Vec<String> =
                    s.features.iter().filter(|x| !SETTABLE.contains(&x.as_str())).cloned().collect();
                s.features =
                    kept.into_iter().chain(f.iter().filter(|x| SETTABLE.contains(&x.as_str())).cloned()).collect();
            }
            macro_rules! set {
                ($field:ident) => {
                    if let Some(v) = &c.$field {
                        s.$field = v.clone();
                    }
                };
            }
            set!(name);
            set!(description);
            for (i, (field, value)) in
                [(&mut s.icon, &c.icon), (&mut s.banner, &c.banner), (&mut s.splash, &c.splash)].into_iter().enumerate()
            {
                if let Some(v) = value {
                    *field = v.as_ref().map(|_| hash(i).unwrap_or_default());
                }
            }
            set!(system_channel_id);
            set!(system_channel_flags);
            set!(rules_channel_id);
            set!(public_updates_channel_id);
            set!(safety_alerts_channel_id);
            set!(afk_channel_id);
            set!(afk_timeout);
            set!(verification_level);
            set!(default_notifications);
            set!(explicit_content_filter);
            set!(preferred_locale);
            set!(premium_progress_bar);
            g.snapshot.guild.name.clone_from(&s.name);
            g.snapshot.guild.features.clone_from(&s.features);
            g.res.get::<SettingsState>().settings = Some(s.clone());
            Ok(s)
        })?;
        for (folder, image) in images {
            if let Some((h, bytes)) = image {
                self.set_url(&guildsmith_core::live::guild_image_url(folder, guild, &h), bytes);
            }
        }
        Ok(updated)
    }

    async fn onboarding(&self, guild: Id) -> PortResult<Onboarding> {
        self.read_guild("onboarding", guild, None, |g| {
            Ok(g.res.peek::<SettingsState>().map(|s| s.onboarding.clone()).unwrap_or_default())
        })
    }

    async fn set_onboarding(&self, guild: Id, o: &Onboarding, reason: &str) -> PortResult<Onboarding> {
        self.write("set_onboarding", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            g.bot_can(Permission::ManageRoles)?;
            if o.enabled && !g.snapshot.guild.features.contains("COMMUNITY") {
                return Err(bad(50035, "onboarding needs a community server"));
            }
            if o.enabled && o.default_channel_ids.len() < 7 {
                return Err(bad(350_000, "Onboarding needs at least 7 default channels"));
            }
            g.res.get::<SettingsState>().onboarding = o.clone();
            Ok(o.clone())
        })
    }

    async fn welcome_screen(&self, guild: Id) -> PortResult<WelcomeScreen> {
        self.read_guild("welcome_screen", guild, None, |g| {
            Ok(g.res.peek::<SettingsState>().map(|s| s.welcome.clone()).unwrap_or_default())
        })
    }

    async fn set_welcome_screen(&self, guild: Id, w: &WelcomeScreen, reason: &str) -> PortResult<WelcomeScreen> {
        self.write("set_welcome_screen", guild, None, Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            if w.channels.len() > 5 {
                return Err(bad(50035, "a welcome screen has at most 5 channels"));
            }
            g.res.get::<SettingsState>().welcome = w.clone();
            Ok(w.clone())
        })
    }

    async fn templates(&self, guild: Id) -> PortResult<Vec<TemplateInfo>> {
        self.read_guild("templates", guild, None, |g| {
            g.bot_can(Permission::ManageGuild)?;
            Ok(g.res.peek::<SettingsState>().map(|s| s.templates.clone()).unwrap_or_default())
        })
    }

    async fn create_template(&self, guild: Id, name: &str, description: Option<&str>) -> PortResult<TemplateInfo> {
        let created = self.write("create_template", guild, None, None, |g, next| {
            g.bot_can(Permission::ManageGuild)?;
            if !g.res.peek::<SettingsState>().is_none_or(|s| s.templates.is_empty()) {
                return Err(bad(30031, "A server can only have a single template"));
            }
            let code = format!("tpl{}", next_id(next));
            let source = serde_json::json!({"name": g.snapshot.guild.name, "roles": [], "channels": []});
            let t = template(&code, name, description, source);
            g.res.get::<SettingsState>().templates.push(t.clone());
            Ok(t)
        })?;
        self.lock().public_templates.0.insert(created.code.clone(), created.clone());
        Ok(created)
    }

    async fn sync_template(&self, guild: Id, code: &str) -> PortResult<TemplateInfo> {
        self.write("sync_template", guild, None, None, |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            let t = g
                .res
                .get::<SettingsState>()
                .templates
                .iter_mut()
                .find(|t| t.code == code)
                .ok_or_else(|| PortError::NotFound(format!("template {code}")))?;
            t.dirty = false;
            "2026-10-19T13:00:00Z".clone_into(&mut t.updated_at);
            Ok(t.clone())
        })
    }

    async fn update_template(
        &self,
        guild: Id,
        code: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> PortResult<TemplateInfo> {
        self.write("update_template", guild, None, None, |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            let t = g
                .res
                .get::<SettingsState>()
                .templates
                .iter_mut()
                .find(|t| t.code == code)
                .ok_or_else(|| PortError::NotFound(format!("template {code}")))?;
            if let Some(n) = name {
                n.clone_into(&mut t.name);
            }
            if let Some(d) = description {
                t.description = Some(d.to_owned());
            }
            Ok(t.clone())
        })
    }

    async fn delete_template(&self, guild: Id, code: &str) -> PortResult<()> {
        self.write("delete_template", guild, None, None, |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            let s = g.res.get::<SettingsState>();
            let before = s.templates.len();
            s.templates.retain(|t| t.code != code);
            if s.templates.len() == before { Err(PortError::NotFound(format!("template {code}"))) } else { Ok(()) }
        })
    }

    async fn get_template(&self, code: &str) -> PortResult<TemplateInfo> {
        self.read("get_template", |st| {
            st.public_templates.0.get(code).cloned().ok_or_else(|| PortError::NotFound(format!("template {code}")))
        })
    }

    async fn integrations(&self, guild: Id) -> PortResult<Vec<IntegrationInfo>> {
        self.read_guild("integrations", guild, None, |g| {
            g.bot_can(Permission::ManageGuild)?;
            Ok(g.res.peek::<SettingsState>().map(|s| s.integrations.clone()).unwrap_or_default())
        })
    }

    async fn delete_integration(&self, guild: Id, id: Id, reason: &str) -> PortResult<()> {
        self.write("delete_integration", guild, Some(id), Some(reason), |g, _| {
            g.bot_can(Permission::ManageGuild)?;
            let s = g.res.get::<SettingsState>();
            let before = s.integrations.len();
            s.integrations.retain(|i| i.id != id);
            if s.integrations.len() == before { Err(PortError::NotFound(format!("integration {id}"))) } else { Ok(()) }
        })
    }

    async fn widget(&self, guild: Id) -> PortResult<WidgetSettings> {
        self.read_guild("widget", guild, None, |g| {
            Ok(g.res.peek::<SettingsState>().map(|s| s.widget.clone()).unwrap_or_default())
        })
    }

    async fn vanity(&self, guild: Id) -> PortResult<Vanity> {
        self.read_guild("vanity", guild, None, |g| {
            Ok(Vanity { code: g.res.peek::<SettingsState>().and_then(|s| s.vanity.clone()), uses: 0 })
        })
    }
}
