//! The opt-in `voice` group (US3): move, mute, deafen and disconnect named members, voice
//! states, stages and voice channel status. REST can't list who is in voice, so members are
//! named (research E5).

use guildsmith_app::bulk::{BulkOutcome, Hooks};
use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::{DiscordPort, StageInfo};
use guildsmith_app::services::moderation::Target;
use guildsmith_app::services::voice::{self, StageAction, VoiceAction, VoiceRow, VoiceStatus};
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{bulk_hooks, outcome_summary, respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MembersParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Up to 25 members (@name or id); members not in voice are reported.
    pub members: Vec<String>,
    /// Move them to this voice or stage channel.
    #[serde(default)]
    pub move_to: Option<String>,
    /// Disconnect them from voice.
    #[serde(default)]
    pub disconnect: bool,
    /// Server-mute (true) or unmute (false).
    #[serde(default)]
    pub mute: Option<bool>,
    /// Server-deafen (true) or undeafen (false).
    #[serde(default)]
    pub deafen: Option<bool>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Member: @name, nickname or id.
    pub member: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StageParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// A stage channel.
    pub channel: String,
    /// start, update (the topic) or end (needs confirm).
    pub action: StageAction,
    /// 1-120 characters.
    #[serde(default)]
    pub topic: Option<String>,
    /// On start: notify members who follow the server.
    #[serde(default)]
    pub notify: bool,
    /// For end: must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StatusParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// A voice channel.
    pub channel: String,
    /// Up to 500 characters; empty clears it.
    pub status: String,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[tool_router(router = voice_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Move, disconnect, mute or deafen up to 25 named members in voice; members not in voice are reported.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<BulkOutcome<Target>>()
    )]
    async fn voice_members(
        &self,
        Parameters(p): Parameters<MembersParams>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let (progress, cancelled, finish) = bulk_hooks(self, &ctx, "voice");
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };
        let action = VoiceAction { move_to: p.move_to, disconnect: p.disconnect, mute: p.mute, deafen: p.deafen };
        let res =
            voice::voice_members(&self.app, p.guild.as_deref(), &p.members, &action, p.reason.as_deref(), &hooks).await;
        finish.done(progress).await;
        wrapped(res, &["name"], BulkOutcome::summary)
    }

    #[tool(
        description = "Show which voice channel a member is in and whether they're muted or deafened.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<VoiceRow>()
    )]
    async fn voice_state(&self, Parameters(p): Parameters<StateParams>) -> CallToolResult {
        let res = voice::voice_state(&self.app, p.guild.as_deref(), &p.member).await;
        respond(res, |v| match &v.channel {
            Some(c) => format!("in {c}"),
            None => "not in voice".to_owned(),
        })
    }

    #[tool(
        description = "Start a stage with a topic, change its topic, or end it (end previews unless confirm).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Option<StageInfo>>>()
    )]
    async fn stage(&self, Parameters(p): Parameters<StageParams>) -> CallToolResult {
        let res = voice::stage(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            p.action,
            p.topic.as_deref(),
            p.notify,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        wrapped(res, &["topic"], |o| {
            outcome_summary(o, |s| if s.is_some() { "stage is live".to_owned() } else { "stage ended".to_owned() })
        })
    }

    #[tool(
        description = "Set or clear a voice channel's status text.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<VoiceStatus>()
    )]
    async fn voice_channel_status(&self, Parameters(p): Parameters<StatusParams>) -> CallToolResult {
        let res =
            voice::voice_channel_status(&self.app, p.guild.as_deref(), &p.channel, &p.status, p.reason.as_deref())
                .await;
        respond(res, |v| if v.status.is_some() { "status set".to_owned() } else { "status cleared".to_owned() })
    }
}
