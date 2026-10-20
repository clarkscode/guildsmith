//! `SettingsPort` for the real Discord API, as JSON through [`crate::raw`] (with `ext::guild`
//! for the settings twilight can't send).

use guildsmith_app::ports::{
    GuildSettings, IntegrationInfo, Onboarding, OnboardingOption, OnboardingPrompt, PortResult, SettingsChange,
    SettingsPort, TemplateInfo, Vanity, WelcomeChannel, WelcomeScreen, WidgetSettings,
};
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Value, json};
use twilight_http::request::Method;

use crate::messages::RawEmoji;
use crate::port::TwilightDiscord;
use crate::{ext, raw};

fn emoji_fields(e: Option<&str>) -> (Value, Value) {
    match e {
        Some(e) if e.parse::<u64>().is_ok() => (json!(e), Value::Null),
        Some(e) => (Value::Null, json!(e)),
        None => (Value::Null, Value::Null),
    }
}

#[derive(Debug, Deserialize)]
struct RawOption {
    id: Id,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    emoji: Option<RawEmoji>,
    #[serde(default)]
    role_ids: Vec<Id>,
    #[serde(default)]
    channel_ids: Vec<Id>,
}

#[derive(Debug, Deserialize)]
struct RawPrompt {
    id: Id,
    title: String,
    #[serde(rename = "type", default)]
    kind: u8,
    #[serde(default)]
    single_select: bool,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    in_onboarding: bool,
    #[serde(default)]
    options: Vec<RawOption>,
}

#[derive(Debug, Deserialize)]
struct RawOnboarding {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    mode: u8,
    #[serde(default)]
    default_channel_ids: Vec<Id>,
    #[serde(default)]
    prompts: Vec<RawPrompt>,
}

impl From<RawOnboarding> for Onboarding {
    fn from(o: RawOnboarding) -> Self {
        Onboarding {
            enabled: o.enabled,
            mode: o.mode,
            default_channel_ids: o.default_channel_ids,
            prompts: o
                .prompts
                .into_iter()
                .map(|p| OnboardingPrompt {
                    id: p.id,
                    title: p.title,
                    kind: p.kind,
                    single_select: p.single_select,
                    required: p.required,
                    in_onboarding: p.in_onboarding,
                    options: p
                        .options
                        .into_iter()
                        .map(|x| OnboardingOption {
                            id: x.id,
                            title: x.title,
                            description: x.description.filter(|d| !d.is_empty()),
                            emoji: x.emoji.map(|e| e.label()).filter(|l| !l.is_empty()),
                            role_ids: x.role_ids,
                            channel_ids: x.channel_ids,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

fn onboarding_body(o: &Onboarding) -> Value {
    let prompts: Vec<Value> = o
        .prompts
        .iter()
        .map(|p| {
            let options: Vec<Value> = p
                .options
                .iter()
                .map(|x| {
                    let (id, name) = emoji_fields(x.emoji.as_deref());
                    json!({"id": x.id, "title": x.title, "description": x.description, "emoji_id": id, "emoji_name": name,
                        "role_ids": x.role_ids, "channel_ids": x.channel_ids})
                })
                .collect();
            json!({"id": p.id, "title": p.title, "type": p.kind, "single_select": p.single_select, "required": p.required,
                "in_onboarding": p.in_onboarding, "options": options})
        })
        .collect();
    json!({"enabled": o.enabled, "mode": o.mode, "default_channel_ids": o.default_channel_ids, "prompts": prompts})
}

#[derive(Debug, Deserialize)]
struct RawWelcomeChannel {
    channel_id: Id,
    #[serde(default)]
    description: String,
    #[serde(default)]
    emoji_id: Option<Id>,
    #[serde(default)]
    emoji_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawWelcome {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    welcome_channels: Vec<RawWelcomeChannel>,
}

#[derive(Debug, Deserialize)]
struct RawTemplate {
    code: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    usage_count: u64,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    is_dirty: Option<bool>,
    #[serde(default)]
    serialized_source_guild: Value,
}

impl From<RawTemplate> for TemplateInfo {
    fn from(t: RawTemplate) -> Self {
        TemplateInfo {
            code: t.code,
            name: t.name,
            description: t.description,
            usage_count: t.usage_count,
            created_at: t.created_at,
            updated_at: t.updated_at,
            dirty: t.is_dirty.unwrap_or(false),
            serialized_source_guild: t.serialized_source_guild,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawAccount {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawIntegration {
    id: Id,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    account: Option<RawAccount>,
}

#[derive(Debug, Deserialize)]
struct RawWidget {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    channel_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct RawVanity {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    uses: u32,
}

impl SettingsPort for TwilightDiscord {
    async fn guild_settings(&self, guild: Id) -> PortResult<GuildSettings> {
        ext::guild::get(self.http().await?, guild).await
    }

    async fn update_guild_settings(
        &self,
        guild: Id,
        change: &SettingsChange,
        reason: &str,
    ) -> PortResult<GuildSettings> {
        ext::guild::update(self.http().await?, guild, change, reason).await
    }

    async fn onboarding(&self, guild: Id) -> PortResult<Onboarding> {
        let http = self.http().await?;
        let o: RawOnboarding =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/onboarding"), &[], None, None).await?;
        Ok(o.into())
    }

    async fn set_onboarding(&self, guild: Id, o: &Onboarding, reason: &str) -> PortResult<Onboarding> {
        let http = self.http().await?;
        let res = raw::send(
            http,
            Method::Put,
            &format!("guilds/{guild}/onboarding"),
            &[],
            Some(&onboarding_body(o)),
            Some(reason),
        )
        .await?;
        Ok(raw::parse::<RawOnboarding>(&res, "set onboarding")?.into())
    }

    async fn welcome_screen(&self, guild: Id) -> PortResult<WelcomeScreen> {
        let http = self.http().await?;
        let settings = ext::guild::get(http, guild).await?;
        let w: RawWelcome =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/welcome-screen"), &[], None, None).await?;
        Ok(WelcomeScreen {
            enabled: settings.features.contains("WELCOME_SCREEN_ENABLED"),
            description: w.description,
            channels: w
                .welcome_channels
                .into_iter()
                .map(|c| WelcomeChannel {
                    channel_id: c.channel_id,
                    description: c.description,
                    emoji: c.emoji_id.map(|i| i.to_string()).or(c.emoji_name),
                })
                .collect(),
        })
    }

    async fn set_welcome_screen(&self, guild: Id, w: &WelcomeScreen, reason: &str) -> PortResult<WelcomeScreen> {
        let http = self.http().await?;
        let channels: Vec<Value> = w
            .channels
            .iter()
            .map(|c| {
                let (id, name) = emoji_fields(c.emoji.as_deref());
                json!({"channel_id": c.channel_id, "description": c.description, "emoji_id": id, "emoji_name": name})
            })
            .collect();
        let body = json!({"enabled": w.enabled, "description": w.description, "welcome_channels": channels});
        raw::send(http, Method::Patch, &format!("guilds/{guild}/welcome-screen"), &[], Some(&body), Some(reason))
            .await?;
        Ok(w.clone())
    }

    async fn templates(&self, guild: Id) -> PortResult<Vec<TemplateInfo>> {
        let http = self.http().await?;
        let l: Vec<RawTemplate> =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/templates"), &[], None, None).await?;
        Ok(l.into_iter().map(TemplateInfo::from).collect())
    }

    async fn create_template(&self, guild: Id, name: &str, description: Option<&str>) -> PortResult<TemplateInfo> {
        let http = self.http().await?;
        let body = json!({"name": name, "description": description});
        let res = raw::send(http, Method::Post, &format!("guilds/{guild}/templates"), &[], Some(&body), None).await?;
        Ok(raw::parse::<RawTemplate>(&res, "create template")?.into())
    }

    async fn sync_template(&self, guild: Id, code: &str) -> PortResult<TemplateInfo> {
        let http = self.http().await?;
        let res = raw::send(http, Method::Put, &format!("guilds/{guild}/templates/{code}"), &[], None, None).await?;
        Ok(raw::parse::<RawTemplate>(&res, "sync template")?.into())
    }

    async fn update_template(
        &self,
        guild: Id,
        code: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> PortResult<TemplateInfo> {
        let http = self.http().await?;
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".to_owned(), json!(n));
        }
        if let Some(d) = description {
            body.insert("description".to_owned(), json!(d));
        }
        let res = raw::send(
            http,
            Method::Patch,
            &format!("guilds/{guild}/templates/{code}"),
            &[],
            Some(&Value::Object(body)),
            None,
        )
        .await?;
        Ok(raw::parse::<RawTemplate>(&res, "update template")?.into())
    }

    async fn delete_template(&self, guild: Id, code: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("guilds/{guild}/templates/{code}"), &[], None, None).await?;
        Ok(())
    }

    async fn get_template(&self, code: &str) -> PortResult<TemplateInfo> {
        let http = self.http().await?;
        let t: RawTemplate =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/templates/{code}"), &[], None, None).await?;
        Ok(t.into())
    }

    async fn integrations(&self, guild: Id) -> PortResult<Vec<IntegrationInfo>> {
        let http = self.http().await?;
        let l: Vec<RawIntegration> =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/integrations"), &[], None, None).await?;
        Ok(l.into_iter()
            .map(|i| IntegrationInfo {
                id: i.id,
                name: i.name,
                kind: i.kind,
                enabled: i.enabled,
                account: i.account.and_then(|a| a.name),
            })
            .collect())
    }

    async fn delete_integration(&self, guild: Id, id: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("guilds/{guild}/integrations/{id}"), &[], None, Some(reason)).await?;
        Ok(())
    }

    async fn widget(&self, guild: Id) -> PortResult<WidgetSettings> {
        let http = self.http().await?;
        let w: RawWidget =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/widget"), &[], None, None).await?;
        Ok(WidgetSettings { enabled: w.enabled, channel_id: w.channel_id })
    }

    async fn vanity(&self, guild: Id) -> PortResult<Vanity> {
        let http = self.http().await?;
        let v: RawVanity =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/vanity-url"), &[], None, None).await?;
        Ok(Vanity { code: v.code, uses: v.uses })
    }
}
