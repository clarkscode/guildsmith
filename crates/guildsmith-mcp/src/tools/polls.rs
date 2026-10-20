//! The opt-in `polls` group (US2): create polls, read results and voters, end polls. Questions,
//! answers and voter names are user-written, so text blocks carry them wrapped as untrusted.

use guildsmith_app::ports::DiscordPort;
use guildsmith_app::services::live::Sent;
use guildsmith_app::services::polls::{self, PollResults};
use guildsmith_core::Id;
use rmcp::handler::server::common::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{respond, wrapped};
use crate::server::GuildsmithServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel: #name, id, or a thread id.
    pub channel: String,
    /// Up to 300 characters.
    pub question: String,
    /// 2-10 answers, each up to 55 characters.
    pub answers: Vec<String>,
    /// How long it runs, in hours (1-768).
    pub duration: u32,
    /// Let members pick more than one answer.
    #[serde(default)]
    pub multiselect: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResultsParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel: #name, id, or a thread id.
    pub channel: String,
    /// The poll's message id.
    pub message: Id,
    /// Also list who voted for this answer (its id or exact text).
    #[serde(default)]
    pub voters_for: Option<String>,
    /// How many voters (default 50, max 100).
    #[serde(default)]
    pub limit: Option<u8>,
    /// Voters with ids above this (the last id of the previous page).
    #[serde(default)]
    pub after: Option<Id>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct EndParams {
    /// Server id or exact name. Optional when exactly one server is allowed.
    #[serde(default)]
    pub guild: Option<String>,
    /// Channel: #name, id, or a thread id.
    pub channel: String,
    /// The poll's message id.
    pub message: Id,
}

const POLL_FIELDS: &[&str] = &["question", "text", "name"];

#[tool_router(router = polls_tools, vis = "pub(crate)")]
impl<P: DiscordPort + 'static> GuildsmithServer<P> {
    #[tool(
        description = "Send a poll: a question, 2-10 answers and a duration in hours.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<Sent>()
    )]
    async fn poll_create(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        let res = polls::poll_create(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            &p.question,
            &p.answers,
            p.duration,
            p.multiselect,
        )
        .await;
        respond(res, |s| format!("poll sent {}", s.url))
    }

    #[tool(
        description = "Read a poll's per-answer counts, and optionally who voted for one answer.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true),
        output_schema = schema_for_output::<PollResults>()
    )]
    async fn poll_results(&self, Parameters(p): Parameters<ResultsParams>) -> CallToolResult {
        let res = polls::poll_results(
            &self.app,
            p.guild.as_deref(),
            &p.channel,
            p.message,
            p.voters_for.as_deref(),
            p.limit,
            p.after,
        )
        .await;
        wrapped(res, POLL_FIELDS, |r| {
            let total: u32 = r.poll.answers.iter().map(|a| a.votes).sum();
            format!("{total} vote(s){}", if r.poll.finalized { ", final" } else { "" })
        })
    }

    #[tool(
        description = "End one of the bot's own polls now.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true),
        output_schema = schema_for_output::<PollResults>()
    )]
    async fn poll_end(&self, Parameters(p): Parameters<EndParams>) -> CallToolResult {
        let res = polls::poll_end(&self.app, p.guild.as_deref(), &p.channel, p.message).await;
        wrapped(res, POLL_FIELDS, |_| "poll ended".to_owned())
    }
}
