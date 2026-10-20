//! `VoicePort`: voice states of named members, moving/muting/deafening/disconnecting them, stage
//! instances and voice channel status (US3, research E5: REST can't list who is in voice).

use std::future::Future;

use guildsmith_core::Id;
use schemars::JsonSchema;
use serde::Serialize;

use super::PortResult;

/// A member's voice state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)] // Discord's voice state flags, one each
pub struct VoiceStateInfo {
    pub user_id: Id,
    /// `None` when not in voice.
    pub channel_id: Option<Id>,
    /// Server-muted / server-deafened.
    pub mute: bool,
    pub deaf: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// On a stage: not a speaker.
    pub suppress: bool,
}

/// A live stage. `topic` is user-written: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StageInfo {
    pub id: Id,
    pub channel_id: Id,
    pub topic: String,
}

pub trait VoicePort: Send + Sync {
    /// A member's voice state; `None` when they aren't in a voice channel.
    fn voice_state(&self, guild: Id, user: Id) -> impl Future<Output = PortResult<Option<VoiceStateInfo>>> + Send;
    /// Moves a member to `channel`, or disconnects them with `None`.
    fn move_member(
        &self,
        guild: Id,
        user: Id,
        channel: Option<Id>,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    /// Server-mutes and/or server-deafens a member (only while they're in voice).
    fn set_voice(
        &self,
        guild: Id,
        user: Id,
        mute: Option<bool>,
        deaf: Option<bool>,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
    fn stage_instance(&self, channel: Id) -> impl Future<Output = PortResult<Option<StageInfo>>> + Send;
    fn create_stage(
        &self,
        channel: Id,
        topic: &str,
        notify: bool,
        reason: &str,
    ) -> impl Future<Output = PortResult<StageInfo>> + Send;
    fn update_stage(
        &self,
        channel: Id,
        topic: &str,
        reason: &str,
    ) -> impl Future<Output = PortResult<StageInfo>> + Send;
    fn delete_stage(&self, channel: Id, reason: &str) -> impl Future<Output = PortResult<()>> + Send;
    /// Sets a voice channel's status (≤ 500 characters), or clears it with `None`.
    fn set_voice_status(
        &self,
        channel: Id,
        status: Option<&str>,
        reason: &str,
    ) -> impl Future<Output = PortResult<()>> + Send;
}
