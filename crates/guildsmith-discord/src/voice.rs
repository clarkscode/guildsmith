//! `VoicePort` for the real Discord API: voice states one member at a time (REST can't list a
//! channel's members, research E5), member moves/mutes, stage instances, and voice status.

use guildsmith_app::ports::{PortError, PortResult, StageInfo, VoicePort, VoiceStateInfo};
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::request::Method;

use crate::port::TwilightDiscord;
use crate::{ext, raw};

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Discord's voice state flags, one each
struct RawVoiceState {
    #[serde(default)]
    channel_id: Option<Id>,
    user_id: Id,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    deaf: bool,
    #[serde(default)]
    self_mute: bool,
    #[serde(default)]
    self_deaf: bool,
    #[serde(default)]
    suppress: bool,
}

#[derive(Debug, Deserialize)]
struct RawStage {
    id: Id,
    channel_id: Id,
    topic: String,
}

impl From<RawStage> for StageInfo {
    fn from(s: RawStage) -> Self {
        StageInfo { id: s.id, channel_id: s.channel_id, topic: s.topic }
    }
}

impl VoicePort for TwilightDiscord {
    async fn voice_state(&self, guild: Id, user: Id) -> PortResult<Option<VoiceStateInfo>> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/voice-states/{user}");
        match raw::json::<(), RawVoiceState>(http, Method::Get, &path, &[], None, None).await {
            Ok(v) if v.channel_id.is_some() => Ok(Some(VoiceStateInfo {
                user_id: v.user_id,
                channel_id: v.channel_id,
                mute: v.mute,
                deaf: v.deaf,
                self_mute: v.self_mute,
                self_deaf: v.self_deaf,
                suppress: v.suppress,
            })),
            Ok(_) | Err(PortError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn move_member(&self, guild: Id, user: Id, channel: Option<Id>, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        let body = json!({"channel_id": channel});
        raw::send(http, Method::Patch, &format!("guilds/{guild}/members/{user}"), &[], Some(&body), Some(reason))
            .await?;
        Ok(())
    }

    async fn set_voice(
        &self,
        guild: Id,
        user: Id,
        mute: Option<bool>,
        deaf: Option<bool>,
        reason: &str,
    ) -> PortResult<()> {
        let http = self.http().await?;
        let mut b = Map::new();
        if let Some(m) = mute {
            b.insert("mute".to_owned(), json!(m));
        }
        if let Some(d) = deaf {
            b.insert("deaf".to_owned(), json!(d));
        }
        let path = format!("guilds/{guild}/members/{user}");
        raw::send(http, Method::Patch, &path, &[], Some(&Value::Object(b)), Some(reason)).await?;
        Ok(())
    }

    async fn stage_instance(&self, channel: Id) -> PortResult<Option<StageInfo>> {
        let http = self.http().await?;
        match raw::json::<(), RawStage>(http, Method::Get, &format!("stage-instances/{channel}"), &[], None, None).await
        {
            Ok(s) => Ok(Some(s.into())),
            Err(PortError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn create_stage(&self, channel: Id, topic: &str, notify: bool, reason: &str) -> PortResult<StageInfo> {
        let http = self.http().await?;
        let body = json!({"channel_id": channel, "topic": topic, "send_start_notification": notify});
        let res = raw::send(http, Method::Post, "stage-instances", &[], Some(&body), Some(reason)).await?;
        Ok(raw::parse::<RawStage>(&res, "start stage")?.into())
    }

    async fn update_stage(&self, channel: Id, topic: &str, reason: &str) -> PortResult<StageInfo> {
        let http = self.http().await?;
        let body = json!({"topic": topic});
        let res = raw::send(http, Method::Patch, &format!("stage-instances/{channel}"), &[], Some(&body), Some(reason))
            .await?;
        Ok(raw::parse::<RawStage>(&res, "update stage")?.into())
    }

    async fn delete_stage(&self, channel: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("stage-instances/{channel}"), &[], None, Some(reason)).await?;
        Ok(())
    }

    async fn set_voice_status(&self, channel: Id, status: Option<&str>, reason: &str) -> PortResult<()> {
        ext::voice::set_status(self.http().await?, channel, status, reason).await
    }
}
