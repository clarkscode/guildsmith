//! `VoicePort` for the fake: who is in voice (set by tests), moving, muting and disconnecting
//! only members who are in voice, stages (topic 1–120) and voice channel status (≤ 500).

use std::collections::BTreeMap;

use guildsmith_core::snapshot::LiveChannelKind;
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission};

use super::{FakeDiscord, FakeGuild, next_id, not_found};
use crate::ports::{PortError, PortResult, StageInfo, VoicePort, VoiceStateInfo};

/// The fake's voice state for one server.
#[derive(Debug, Clone, Default)]
pub struct VoiceState {
    pub states: BTreeMap<Id, VoiceStateInfo>,
    pub stages: BTreeMap<Id, StageInfo>,
    pub statuses: BTreeMap<Id, String>,
}

fn not_in_voice() -> PortError {
    PortError::Discord { status: 400, code: Some(40032), message: "Target user is not connected to voice.".to_owned() }
}

fn bad(message: &str) -> PortError {
    PortError::Discord { status: 400, code: Some(50035), message: message.to_owned() }
}

impl FakeGuild {
    fn kind_of(&self, channel: Id) -> Option<ChannelKind> {
        match self.snapshot.channel(channel)?.kind {
            LiveChannelKind::Known(k) => Some(k),
            _ => None,
        }
    }
}

impl FakeDiscord {
    /// Puts a member in a voice channel, as if they joined it.
    pub fn join_voice(&self, guild: Id, user: Id, channel: Id) {
        self.edit_res::<VoiceState>(guild, |v| {
            v.states.insert(
                user,
                VoiceStateInfo {
                    user_id: user,
                    channel_id: Some(channel),
                    mute: false,
                    deaf: false,
                    self_mute: false,
                    self_deaf: false,
                    suppress: false,
                },
            );
        });
    }
}

// In-memory: the trait is async, but nothing here needs to await.
#[allow(clippy::unused_async_trait_impl)]
impl VoicePort for FakeDiscord {
    async fn voice_state(&self, guild: Id, user: Id) -> PortResult<Option<VoiceStateInfo>> {
        self.read_guild("voice_state", guild, Some(user), |g| {
            Ok(g.res.peek::<VoiceState>().and_then(|v| v.states.get(&user)).filter(|s| s.channel_id.is_some()).cloned())
        })
    }

    async fn move_member(&self, guild: Id, user: Id, channel: Option<Id>, reason: &str) -> PortResult<()> {
        self.write("move_member", guild, Some(user), Some(reason), |g, _| {
            g.bot_can(Permission::MoveMembers)?;
            if let Some(c) = channel
                && !matches!(g.kind_of(c), Some(ChannelKind::Voice | ChannelKind::Stage))
            {
                return Err(bad("the target isn't a voice or stage channel"));
            }
            let v = g.res.get::<VoiceState>();
            let s = v.states.get_mut(&user).filter(|s| s.channel_id.is_some()).ok_or_else(not_in_voice)?;
            s.channel_id = channel;
            Ok(())
        })
    }

    async fn set_voice(
        &self,
        guild: Id,
        user: Id,
        mute: Option<bool>,
        deaf: Option<bool>,
        reason: &str,
    ) -> PortResult<()> {
        self.write("set_voice", guild, Some(user), Some(reason), |g, _| {
            if mute.is_some() {
                g.bot_can(Permission::MuteMembers)?;
            }
            if deaf.is_some() {
                g.bot_can(Permission::DeafenMembers)?;
            }
            let v = g.res.get::<VoiceState>();
            let s = v.states.get_mut(&user).filter(|s| s.channel_id.is_some()).ok_or_else(not_in_voice)?;
            if let Some(m) = mute {
                s.mute = m;
            }
            if let Some(d) = deaf {
                s.deaf = d;
            }
            Ok(())
        })
    }

    async fn stage_instance(&self, channel: Id) -> PortResult<Option<StageInfo>> {
        let guild = self.guild_of_channel(channel)?;
        self.read_guild("stage_instance", guild, Some(channel), |g| {
            Ok(g.res.peek::<VoiceState>().and_then(|v| v.stages.get(&channel)).cloned())
        })
    }

    async fn create_stage(&self, channel: Id, topic: &str, _notify: bool, reason: &str) -> PortResult<StageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("create_stage", guild, Some(channel), Some(reason), |g, next| {
            g.bot_can(Permission::ManageChannels)?;
            if g.kind_of(channel) != Some(ChannelKind::Stage) {
                return Err(bad("not a stage channel"));
            }
            if !(1..=120).contains(&topic.chars().count()) {
                return Err(bad("the topic is 1–120 characters"));
            }
            let v = g.res.get::<VoiceState>();
            if v.stages.contains_key(&channel) {
                return Err(PortError::Discord {
                    status: 400,
                    code: Some(150_006),
                    message: "A stage is already live".to_owned(),
                });
            }
            let s = StageInfo { id: next_id(next), channel_id: channel, topic: topic.to_owned() };
            v.stages.insert(channel, s.clone());
            Ok(s)
        })
    }

    async fn update_stage(&self, channel: Id, topic: &str, reason: &str) -> PortResult<StageInfo> {
        let guild = self.guild_of_channel(channel)?;
        self.write("update_stage", guild, Some(channel), Some(reason), |g, _| {
            g.bot_can(Permission::ManageChannels)?;
            if !(1..=120).contains(&topic.chars().count()) {
                return Err(bad("the topic is 1–120 characters"));
            }
            let s = g
                .res
                .get::<VoiceState>()
                .stages
                .get_mut(&channel)
                .ok_or_else(|| not_found("stage instance", channel))?;
            topic.clone_into(&mut s.topic);
            Ok(s.clone())
        })
    }

    async fn delete_stage(&self, channel: Id, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("delete_stage", guild, Some(channel), Some(reason), |g, _| {
            g.bot_can(Permission::ManageChannels)?;
            g.res
                .get::<VoiceState>()
                .stages
                .remove(&channel)
                .map(|_| ())
                .ok_or_else(|| not_found("stage instance", channel))
        })
    }

    async fn set_voice_status(&self, channel: Id, status: Option<&str>, reason: &str) -> PortResult<()> {
        let guild = self.guild_of_channel(channel)?;
        self.write("set_voice_status", guild, Some(channel), Some(reason), |g, _| {
            g.bot_can(Permission::SetVoiceChannelStatus)?;
            if g.kind_of(channel) != Some(ChannelKind::Voice) {
                return Err(bad("only voice channels have a status"));
            }
            if status.is_some_and(|s| s.chars().count() > 500) {
                return Err(bad("the status is at most 500 characters"));
            }
            let v = g.res.get::<VoiceState>();
            match status {
                Some(s) => {
                    v.statuses.insert(channel, s.to_owned());
                }
                None => {
                    v.statuses.remove(&channel);
                }
            }
            Ok(())
        })
    }
}
