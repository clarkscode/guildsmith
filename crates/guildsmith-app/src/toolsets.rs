//! Tool groups (data-model §1, research E1/E13).
//!
//! The operator registers groups at startup (`GUILDSMITH_TOOLSETS`); they appear in
//! `tools/list`. The agent can unlock more groups during a session through the `toolsets`
//! dispatcher tool, and call their tools through `tool_call`. The operator's block list
//! (`GUILDSMITH_TOOLSETS_BLOCK`) can't be overridden. Only `unlocked` can change after startup,
//! and every change is logged.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::UNIX_EPOCH;

use guildsmith_core::{Permission, PermissionSet};
use schemars::JsonSchema;
use serde::Serialize;

use crate::date;
use crate::policy::{Refusal, RefusalKind};
use crate::ports::Clock;
use crate::preflight;

/// A named set of agent tools. Only `core` is registered by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Toolset {
    Core,
    Moderation,
    Messages,
    Threads,
    Polls,
    Events,
    Invites,
    Voice,
    Automod,
    Webhooks,
    Expressions,
    Settings,
    Raw,
}

impl Toolset {
    /// Every group, in registration order.
    pub const ALL: &'static [Toolset] = &[
        Toolset::Core,
        Toolset::Moderation,
        Toolset::Messages,
        Toolset::Threads,
        Toolset::Polls,
        Toolset::Events,
        Toolset::Invites,
        Toolset::Voice,
        Toolset::Automod,
        Toolset::Webhooks,
        Toolset::Expressions,
        Toolset::Settings,
        Toolset::Raw,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Toolset::Core => "core",
            Toolset::Moderation => "moderation",
            Toolset::Messages => "messages",
            Toolset::Threads => "threads",
            Toolset::Polls => "polls",
            Toolset::Events => "events",
            Toolset::Invites => "invites",
            Toolset::Voice => "voice",
            Toolset::Automod => "automod",
            Toolset::Webhooks => "webhooks",
            Toolset::Expressions => "expressions",
            Toolset::Settings => "settings",
            Toolset::Raw => "raw",
        }
    }

    /// One line on what the group is for (shown by `toolsets list`).
    #[must_use]
    pub const fn purpose(self) -> &'static str {
        match self {
            Toolset::Core => "Server structure, spec plan/apply, roles, channels, permissions, members, messages.",
            Toolset::Moderation => "Bans, timeouts, kicks, prune, message purge, bulk roles, audit log, raid lockdown.",
            Toolset::Messages => "Compose, edit, delete, pin, crosspost, react to and search messages.",
            Toolset::Threads => "Threads, forum and media posts, and forum tags.",
            Toolset::Polls => "Create polls, read results, end polls.",
            Toolset::Events => "Scheduled events and their interested users.",
            Toolset::Invites => "Create, list and revoke invites.",
            Toolset::Voice => "Move, mute and disconnect named members; stage instances; voice channel status.",
            Toolset::Automod => "Discord's built-in auto-moderation rules.",
            Toolset::Webhooks => "Create, list, edit, delete and post through webhooks.",
            Toolset::Expressions => "Custom emoji, stickers and soundboard sounds.",
            Toolset::Settings => "Server settings, onboarding, welcome screen, templates, integrations.",
            Toolset::Raw => "Direct Discord API requests for anything the dedicated tools don't cover.",
        }
    }

    /// The group's tools, in `tools/list` order.
    #[must_use]
    pub const fn tools(self) -> &'static [&'static str] {
        match self {
            Toolset::Core => &[
                "guilds_list",
                "guild_overview",
                "refresh",
                "spec_export",
                "spec_validate",
                "spec_plan",
                "spec_apply",
                "roles_list",
                "role_upsert",
                "role_delete",
                "channels_tree",
                "channel_upsert",
                "channel_delete",
                "channels_reorder",
                "permissions_set",
                "permissions_explain",
                "members_find",
                "member_roles",
                "message_send",
                "messages_read",
                "bot_preflight",
                "toolsets",
                "tool_call",
            ],
            Toolset::Moderation => &[
                "member_ban",
                "ban_remove",
                "bans_list",
                "member_timeout",
                "member_kick",
                "members_prune",
                "messages_purge",
                "roles_bulk_assign",
                "audit_log",
                "raid_lockdown",
            ],
            Toolset::Messages => &[
                "message_compose",
                "message_edit",
                "message_delete",
                "message_pin",
                "pins_list",
                "message_crosspost",
                "reactions",
                "reactions_list",
                "messages_search",
            ],
            Toolset::Threads => {
                &["thread_create", "thread_update", "thread_delete", "threads_list", "thread_members", "forum_tags"]
            }
            Toolset::Polls => &["poll_create", "poll_results", "poll_end"],
            Toolset::Events => &["event_create", "event_update", "event_delete", "events_list", "event_users"],
            Toolset::Invites => &["invite_create", "invites_list", "invite_revoke", "invite_target_users"],
            Toolset::Voice => &["voice_members", "voice_state", "stage", "voice_channel_status"],
            Toolset::Automod => &["automod_rules_list", "automod_rule_upsert", "automod_rule_delete"],
            Toolset::Webhooks => {
                &["webhook_create", "webhooks_list", "webhook_update", "webhook_delete", "webhook_send"]
            }
            Toolset::Expressions => {
                &["expressions_list", "expression_create", "expression_update", "expression_delete"]
            }
            Toolset::Settings => &[
                "settings_get",
                "settings_update",
                "onboarding_update",
                "welcome_screen_update",
                "templates",
                "template_import",
                "integration_remove",
            ],
            Toolset::Raw => &["discord_request"],
        }
    }

    /// The group a tool belongs to.
    #[must_use]
    pub fn of_tool(tool: &str) -> Option<Toolset> {
        Toolset::ALL.iter().copied().find(|t| t.tools().contains(&tool))
    }

    /// The server permissions the group's tools need (research E13), from each tool's needs.
    #[must_use]
    pub fn required_permissions(self) -> PermissionSet {
        let mut set = PermissionSet::EMPTY;
        for tool in self.tools() {
            for p in preflight::tool_needs(tool).unwrap_or_default() {
                set.insert(*p);
            }
        }
        // Granted per request: the tools check them when the option is used.
        let extra: &[Permission] = match self {
            Toolset::Moderation => &[Permission::ManageGuild],
            Toolset::Threads => &[Permission::CreatePrivateThreads, Permission::SendMessagesInThreads],
            Toolset::Invites => &[Permission::ManageRoles, Permission::ManageChannels],
            Toolset::Voice => &[Permission::MuteMembers, Permission::DeafenMembers, Permission::ManageChannels],
            Toolset::Automod => &[Permission::ModerateMembers],
            Toolset::Expressions => &[Permission::ManageGuildExpressions],
            _ => &[],
        };
        for p in extra {
            set.insert(*p);
        }
        set
    }

    /// Privileged gateway intents the group needs, as Discord names them.
    #[must_use]
    pub const fn required_intents(self) -> &'static [&'static str] {
        match self {
            Toolset::Messages => &["MESSAGE_CONTENT"],
            _ => &[],
        }
    }
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An unknown toolset name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown toolset `{name}`; valid toolsets: {valid}")]
pub struct UnknownToolset {
    pub name: String,
    pub valid: String,
}

fn valid_names() -> String {
    let mut names: Vec<&str> = Toolset::ALL.iter().map(|t| t.name()).collect();
    names.extend(["all", "default"]);
    names.join(", ")
}

impl FromStr for Toolset {
    type Err = UnknownToolset;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Toolset::ALL
            .iter()
            .copied()
            .find(|t| t.name() == s)
            .ok_or_else(|| UnknownToolset { name: s.to_owned(), valid: valid_names() })
    }
}

/// Parses `GUILDSMITH_TOOLSETS` (`core,moderation`, `default`, `all`). `core` is always
/// included.
pub fn parse_toolsets(raw: &str) -> Result<BTreeSet<Toolset>, UnknownToolset> {
    let mut set = BTreeSet::from([Toolset::Core]);
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "default" => {}
            "all" => set.extend(Toolset::ALL.iter().copied()),
            other => {
                set.insert(other.parse()?);
            }
        }
    }
    Ok(set)
}

/// A block list that can't be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockError {
    #[error(transparent)]
    Unknown(#[from] UnknownToolset),
    #[error("the core toolset can't be blocked")]
    Core,
}

/// Parses `GUILDSMITH_TOOLSETS_BLOCK`: a comma list of optional groups, or `all` for every
/// optional group.
pub fn parse_blocked(raw: &str) -> Result<BTreeSet<Toolset>, BlockError> {
    let mut set = BTreeSet::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "all" => set.extend(Toolset::ALL.iter().copied().filter(|t| *t != Toolset::Core)),
            "core" => return Err(BlockError::Core),
            other => {
                set.insert(other.parse::<Toolset>()?);
            }
        }
    }
    Ok(set)
}

/// Who turned a group on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnabledBy {
    /// At startup, from `GUILDSMITH_TOOLSETS`.
    Operator,
    /// During the session, through `toolsets(enable)`.
    Agent,
}

/// One enablement, as logged (FR-003c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Enablement {
    /// RFC 3339 (UTC).
    pub at: String,
    pub toolset: Toolset,
    pub by: EnabledBy,
}

/// Which groups are registered, unlocked and blocked in this process.
///
/// `registered` and `blocked` are fixed at startup. The agent can only add to `unlocked`
/// (through [`ToolsetState::enable`]); nothing removes a group once unlocked.
#[derive(Debug)]
pub struct ToolsetState {
    registered: BTreeSet<Toolset>,
    blocked: BTreeSet<Toolset>,
    unlocked: RwLock<BTreeSet<Toolset>>,
    log: Mutex<Vec<Enablement>>,
}

fn now_rfc3339(clock: &dyn Clock) -> String {
    date::rfc3339(clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

impl ToolsetState {
    /// The state at startup. A blocked group is never registered, even when listed in
    /// `GUILDSMITH_TOOLSETS` (the block takes precedence). Each registered optional group is
    /// logged as enabled by the operator.
    #[must_use]
    pub fn new(registered: &BTreeSet<Toolset>, blocked: &BTreeSet<Toolset>, clock: &dyn Clock) -> Self {
        let registered: BTreeSet<Toolset> = registered.difference(blocked).copied().collect();
        let at = now_rfc3339(clock);
        let log: Vec<Enablement> = registered
            .iter()
            .filter(|t| **t != Toolset::Core)
            .map(|t| Enablement { at: at.clone(), toolset: *t, by: EnabledBy::Operator })
            .collect();
        for e in &log {
            log_enablement(e);
        }
        ToolsetState {
            unlocked: RwLock::new(registered.clone()),
            registered,
            blocked: blocked.clone(),
            log: Mutex::new(log),
        }
    }

    /// Groups listed in `tools/list`.
    #[must_use]
    pub fn registered(&self) -> &BTreeSet<Toolset> {
        &self.registered
    }

    /// Groups the operator blocked.
    #[must_use]
    pub fn blocked(&self) -> &BTreeSet<Toolset> {
        &self.blocked
    }

    /// Groups whose tools can be called: registered plus those the agent enabled.
    #[must_use]
    pub fn unlocked(&self) -> BTreeSet<Toolset> {
        self.unlocked.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    #[must_use]
    pub fn is_unlocked(&self, t: Toolset) -> bool {
        self.unlocked.read().unwrap_or_else(PoisonError::into_inner).contains(&t)
    }

    /// Unlocks a group for the rest of the process. Refused if the operator blocked it.
    /// Enabling an unlocked group again changes nothing and logs nothing.
    pub fn enable(&self, t: Toolset, clock: &dyn Clock) -> Result<Option<Enablement>, Refusal> {
        if self.blocked.contains(&t) {
            return Err(Refusal::new(
                RefusalKind::Blocked,
                format!("the {t} toolset is blocked by the operator"),
                Some("the operator can unblock it in GUILDSMITH_TOOLSETS_BLOCK; agents can't"),
            ));
        }
        let mut unlocked = self.unlocked.write().unwrap_or_else(PoisonError::into_inner);
        if !unlocked.insert(t) {
            return Ok(None);
        }
        let e = Enablement { at: now_rfc3339(clock), toolset: t, by: EnabledBy::Agent };
        log_enablement(&e);
        self.log.lock().unwrap_or_else(PoisonError::into_inner).push(e.clone());
        Ok(Some(e))
    }

    /// Every enablement so far, in order (operator ones first).
    #[must_use]
    pub fn enablements(&self) -> Vec<Enablement> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

fn log_enablement(e: &Enablement) {
    let by = match e.by {
        EnabledBy::Operator => "operator",
        EnabledBy::Agent => "agent",
    };
    tracing::info!(target: "guildsmith::toolsets", at = %e.at, toolset = e.toolset.name(), by, "toolset enabled");
}
