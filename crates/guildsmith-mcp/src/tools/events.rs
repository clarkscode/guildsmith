//! The opt-in `events` group (US3): scheduled events and interested users. Event names,
//! descriptions and member names are user-written, so text blocks carry them wrapped.

use guildsmith_app::confirm::Outcome;
use guildsmith_app::ports::{DiscordPort, UserInfo};
use guildsmith_app::services::events::{self, EventInput, EventRow, EventStatus, LocationInput, RecurrenceInput};
use guildsmith_app::services::live::Deleted;
use guildsmith_core::Id;
use guildsmith_core::asset::Asset;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{quiet_outcome_summary, wrapped};
use crate::server::GuildsmithServer;

const EVENT_FIELDS: &[&str] = &["name", "description", "would"];

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// 1-100 characters.
    pub name: String,
    /// When it starts: 2026-10-23T20:00 (read in tz, UTC if none) or with an offset.
    pub start: String,
    /// When it ends (needed for external events).
    #[serde(default)]
    pub end: Option<String>,
    /// Offset for times without one, e.g. -07:00. Default UTC.
    #[serde(default)]
    pub tz: Option<String>,
    /// {voice: channel}, {stage: channel} or {external: place}.
    pub location: LocationInput,
    /// Up to 1000 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// How it repeats: {frequency: weekly, weekday: [friday]}.
    #[serde(default)]
    pub recurrence: Option<RecurrenceInput>,
    /// Cover image: {url}, {data} or {file}.
    #[serde(default)]
    pub image: Option<Asset>,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Event id or exact name.
    pub event: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    /// Offset for times without one, e.g. -07:00. Default UTC.
    #[serde(default)]
    pub tz: Option<String>,
    #[serde(default)]
    pub location: Option<LocationInput>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceInput>,
    #[serde(default)]
    pub image: Option<Asset>,
    /// active (start it), completed (end it) or canceled (needs confirm).
    #[serde(default)]
    pub status: Option<EventStatus>,
    /// For canceling: must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Event id or exact name.
    pub event: String,
    /// Must be true to act; otherwise returns a preview.
    #[serde(default)]
    pub confirm: bool,
    /// Note appended to the audit-log reason.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GuildOnly {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UsersParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Event id or exact name.
    pub event: String,
    /// How many (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u8>,
    /// Users with ids above this (the last id of the previous page).
    #[serde(default)]
    pub after: Option<Id>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct EventsOutput {
    pub events: Vec<EventRow>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct UsersOutput {
    pub users: Vec<UserInfo>,
}

#[tool_router(router = events_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Schedule an event in a voice or stage channel or at an external place; times without an offset are UTC.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<EventRow>()
    )]
    async fn event_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let input = EventInput {
            name: Some(p.name),
            description: p.description,
            start: Some(p.start),
            end: p.end,
            tz: p.tz,
            location: Some(p.location),
            recurrence: p.recurrence,
            image: p.image,
        };
        let res = events::event_create(&self.app, p.guild.as_deref(), &input, p.reason.as_deref()).await;
        wrapped(res, EVENT_FIELDS, |e| format!("scheduled {} for {}", e.id, e.when))
    }

    #[tool(
        description = "Change an event, or start, end or cancel it (cancel previews unless confirm).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<EventRow>>()
    )]
    async fn event_update(&self, Parameters(p): Parameters<UpdateParams>) -> CallToolResult {
        let input = EventInput {
            name: p.name,
            description: p.description,
            start: p.start,
            end: p.end,
            tz: p.tz,
            location: p.location,
            recurrence: p.recurrence,
            image: p.image,
        };
        let res = events::event_update(
            &self.app,
            p.guild.as_deref(),
            &p.event,
            &input,
            p.status,
            p.confirm,
            p.reason.as_deref(),
        )
        .await;
        wrapped(res, EVENT_FIELDS, |o| quiet_outcome_summary(o, |e| format!("event {} is {}", e.id, e.status)))
    }

    #[tool(
        description = "Delete an event; previews unless confirm.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Outcome<Deleted>>()
    )]
    async fn event_delete(&self, Parameters(p): Parameters<DeleteParams>) -> CallToolResult {
        let res = events::event_delete(&self.app, p.guild.as_deref(), &p.event, p.confirm, p.reason.as_deref()).await;
        wrapped(res, EVENT_FIELDS, |o| quiet_outcome_summary(o, |d| format!("deleted {}", d.deleted)))
    }

    #[tool(
        description = "List scheduled events with their status, time (UTC) and interested count.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<EventsOutput>()
    )]
    async fn events_list(&self, Parameters(p): Parameters<GuildOnly>) -> CallToolResult {
        let res = events::events_list(&self.app, p.guild.as_deref()).await.map(|events| EventsOutput { events });
        wrapped(res, EVENT_FIELDS, |o| format!("{} event(s)", o.events.len()))
    }

    #[tool(
        description = "List members interested in an event, in pages.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<UsersOutput>()
    )]
    async fn event_users(&self, Parameters(p): Parameters<UsersParams>) -> CallToolResult {
        let res = events::event_users(&self.app, p.guild.as_deref(), &p.event, p.limit, p.after)
            .await
            .map(|users| UsersOutput { users });
        wrapped(res, &["name"], |o| format!("{} interested", o.users.len()))
    }
}
