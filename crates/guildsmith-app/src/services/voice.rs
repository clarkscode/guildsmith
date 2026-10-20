//! The `voice` group (US3, FR-027): move, mute, deafen and disconnect named members, read a
//! member's voice state, run stages, and set voice channel status.
//!
//! REST can't list who is in a voice channel (research E5), so voice actions take the members
//! by name; members who aren't in voice are reported, not skipped silently.

use guildsmith_core::Id;
use guildsmith_core::Permission;
use guildsmith_core::snapshot::{LiveChannelKind, Snapshot};
use guildsmith_core::spec::ChannelKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;
use crate::bulk::{BatchResult, BulkAction, BulkOutcome, Hooks, Problem, SINGLE};
use crate::confirm::{self, Outcome};
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, PortError, StageInfo};
use crate::preflight::{self, PreflightProblem};
use crate::resolve;
use crate::services::live::{find_member, invalid, refuse_if, resolved, server, writable};
use crate::services::moderation::Target;

/// At most this many members per `voice_members` call.
pub const MAX_MEMBERS: usize = 25;

fn voice_channel(s: &Snapshot, input: &str, kinds: &[ChannelKind]) -> Result<Id, ServiceError> {
    let id = resolved(resolve::resolve_channel(s, input), "channel", input)?;
    match s.channel(id).map(|c| c.kind) {
        Some(LiveChannelKind::Known(k)) if kinds.contains(&k) => Ok(id),
        _ => {
            let names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
            Err(invalid(format!("`{input}` isn't a {} channel", names.join(" or "))))
        }
    }
}

fn need(s: &Snapshot, p: Permission, why: &str) -> Vec<PreflightProblem> {
    let admin = s.bot.permissions.contains(Permission::Administrator) || s.bot.user_id == s.guild.owner_id;
    if admin || s.bot.permissions.contains(p) {
        Vec::new()
    } else {
        vec![PreflightProblem::MissingPermission { perm: p.name().to_owned(), needed_by: why.to_owned() }]
    }
}

/// What to do to each member.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceAction {
    pub move_to: Option<String>,
    pub disconnect: bool,
    pub mute: Option<bool>,
    pub deafen: Option<bool>,
}

/// Moves, disconnects, mutes or deafens up to 25 named members. Members who aren't in voice
/// are reported as failed; the rest go ahead.
pub async fn voice_members<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    members: &[String],
    action: &VoiceAction,
    reason: Option<&str>,
    hooks: &Hooks<'_>,
) -> Result<BulkOutcome<Target>, ServiceError> {
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return Err(invalid("give 1–25 members (Discord can't list who is in a voice channel, so name them)"));
    }
    if action.move_to.is_some() && action.disconnect {
        return Err(invalid("use move_to or disconnect, not both"));
    }
    if action.move_to.is_none() && !action.disconnect && action.mute.is_none() && action.deafen.is_none() {
        return Err(invalid("say what to do: move_to, disconnect, mute and/or deafen"));
    }
    let (gid, s) = writable(app, guild).await?;
    let target = action
        .move_to
        .as_deref()
        .map(|c| voice_channel(&s, c, &[ChannelKind::Voice, ChannelKind::Stage]))
        .transpose()?;
    let mut problems = Vec::new();
    if target.is_some() || action.disconnect {
        problems.extend(need(&s, Permission::MoveMembers, "voice_members move_to/disconnect"));
    }
    if action.mute.is_some() {
        problems.extend(need(&s, Permission::MuteMembers, "voice_members mute"));
    }
    if action.deafen.is_some() {
        problems.extend(need(&s, Permission::DeafenMembers, "voice_members deafen"));
    }
    refuse_if(&problems)?;
    let mut list = Vec::new();
    let mut absent = Vec::new();
    for input in members {
        let m = find_member(app, gid, input).await?;
        let t = Target { id: m.id, name: m.nick.clone().or(m.display_name.clone()).unwrap_or(m.username.clone()) };
        if list.contains(&t) {
            continue;
        }
        if app.port.voice_state(gid, m.id).await?.is_none() {
            absent.push(t.clone());
        }
        list.push(t);
    }
    let mut bulk = BulkAction::new("voice change", list, SINGLE);
    for t in absent {
        bulk = bulk.with_problem(t, Problem::NotInVoice);
    }
    let label = target.and_then(|c| s.channel(c)).map(|c| c.name.clone());
    let out = bulk
        .run(
            |batch: Vec<Target>| {
                let label = label.clone();
                async move {
                    let t = batch.into_iter().next().expect("batch of one");
                    let who = format!("{} ({})", t.name, t.id);
                    let mut done: Result<(), PortError> = Ok(());
                    if let Some(c) = target {
                        let r = audit_reason(&format!("move {who} to {}", label.unwrap_or_default()), reason);
                        done = app.port.move_member(gid, t.id, Some(c), &r).await;
                    } else if action.disconnect {
                        done = app
                            .port
                            .move_member(gid, t.id, None, &audit_reason(&format!("disconnect {who}"), reason))
                            .await;
                    }
                    if done.is_ok() && (action.mute.is_some() || action.deafen.is_some()) {
                        let r = audit_reason(&format!("voice mute/deafen {who}"), reason);
                        done = app.port.set_voice(gid, t.id, action.mute, action.deafen, &r).await;
                    }
                    match done {
                        Err(e @ (PortError::Unauthorized | PortError::Network(_))) => BatchResult::Fatal(e.to_string()),
                        other => BatchResult::single(t, other.map_err(|e| e.to_string())),
                    }
                }
            },
            hooks,
        )
        .await;
    Ok(out)
}

/// A member's voice state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)] // Discord's voice state flags, one each
pub struct VoiceRow {
    pub member: Id,
    /// `null` when not in voice.
    pub channel: Option<String>,
    pub mute: bool,
    pub deaf: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// On a stage: in the audience.
    pub suppress: bool,
}

/// Where a member is in voice, and whether they're muted or deafened.
pub async fn voice_state<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    member: &str,
) -> Result<VoiceRow, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let m = find_member(app, gid, member).await?;
    Ok(match app.port.voice_state(gid, m.id).await? {
        Some(v) => VoiceRow {
            member: m.id,
            channel: v.channel_id.map(|c| s.channel(c).map_or_else(|| c.to_string(), |c| c.name.clone())),
            mute: v.mute,
            deaf: v.deaf,
            self_mute: v.self_mute,
            self_deaf: v.self_deaf,
            suppress: v.suppress,
        },
        None => VoiceRow {
            member: m.id,
            channel: None,
            mute: false,
            deaf: false,
            self_mute: false,
            self_deaf: false,
            suppress: false,
        },
    })
}

/// What `stage` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageAction {
    Start,
    Update,
    End,
}

/// Starts a stage with a topic (1–120 characters), changes its topic, or ends it (after
/// confirmation).
#[allow(clippy::too_many_arguments)] // mirrors the tool's inputs
pub async fn stage<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    action: StageAction,
    topic: Option<&str>,
    notify: bool,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Option<StageInfo>>, ServiceError> {
    let topic = topic.map(str::trim);
    if action != StageAction::End && topic.is_none_or(|t| !(1..=120).contains(&t.chars().count())) {
        return Err(invalid("a stage topic is 1–120 characters"));
    }
    let (_, s) = writable(app, guild).await?;
    let cid = voice_channel(&s, channel, &[ChannelKind::Stage])?;
    refuse_if(&preflight::check_tools(&s, ["stage"]))?;
    let live = app.port.stage_instance(cid).await?;
    let name = s.channel(cid).map_or_else(String::new, |c| c.name.clone());
    match action {
        StageAction::Start => {
            if live.is_some() {
                return Err(invalid(format!("a stage is already live in {name}; use update or end")));
            }
            let t = topic.expect("checked");
            let st =
                app.port.create_stage(cid, t, notify, &audit_reason(&format!("start stage in {name}"), reason)).await?;
            Ok(Outcome::Done(Some(st)))
        }
        StageAction::Update => {
            if live.is_none() {
                return Err(invalid(format!("no stage is live in {name}; use start")));
            }
            let t = topic.expect("checked");
            let st =
                app.port.update_stage(cid, t, &audit_reason(&format!("change stage topic in {name}"), reason)).await?;
            Ok(Outcome::Done(Some(st)))
        }
        StageAction::End => {
            if live.is_none() {
                return Err(invalid(format!("no stage is live in {name}")));
            }
            confirm::guarded(
                confirmed,
                vec![format!("end the stage in {name}; everyone on it is moved to the audience")],
                || async {
                    app.port.delete_stage(cid, &audit_reason(&format!("end stage in {name}"), reason)).await?;
                    Ok(None)
                },
            )
            .await
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct VoiceStatus {
    pub channel: Id,
    /// `null` when cleared.
    pub status: Option<String>,
}

/// Sets a voice channel's status (≤ 500 characters); an empty status clears it.
pub async fn voice_channel_status<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    status: &str,
    reason: Option<&str>,
) -> Result<VoiceStatus, ServiceError> {
    if status.chars().count() > 500 {
        return Err(invalid("a voice channel status is at most 500 characters"));
    }
    let (_, s) = writable(app, guild).await?;
    let cid = voice_channel(&s, channel, &[ChannelKind::Voice])?;
    refuse_if(&preflight::check_tools(&s, ["voice_channel_status"]))?;
    let status = Some(status.trim()).filter(|t| !t.is_empty());
    let what = if status.is_some() { "set" } else { "clear" };
    app.port.set_voice_status(cid, status, &audit_reason(&format!("{what} voice status"), reason)).await?;
    Ok(VoiceStatus { channel: cid, status: status.map(str::to_owned) })
}
