//! Plan operations (data-model §3). Each op is one Discord request, except reorders, which the
//! apply service turns into one positions request on a fresh view of the server.
//!
//! Items that don't exist yet are referred to by a symbolic key (`Ref::New`), which the apply
//! service replaces with the id Discord returns when the create runs.

use std::fmt;

use serde::Serialize;

use std::collections::BTreeSet;

use serde_json::Value;

use crate::asset::Asset;
use crate::id::Id;
use crate::live::ExpressionKind;
use crate::permissions::PermissionSet;
use crate::snapshot::{LiveChannelKind, OverwriteKind};
use crate::spec::automod::AutomodRule;
use crate::spec::expressions::Volume;
use crate::spec::{ForumTag, SpecPath};

/// An existing item, or one created earlier in the same plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ref {
    Id(Id),
    /// The key of a `CreateRole` / `CreateChannel` op in the same plan.
    New(String),
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Id(id) => write!(f, "{id}"),
            Ref::New(key) => write!(f, "new {key}"),
        }
    }
}

/// Whether an op can lose data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Safe,
    Destructive,
}

/// One changed field, for people: `old → new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old: String,
    pub new: String,
}

impl fmt::Display for FieldChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} → {}", self.field, self.old, self.new)
    }
}

/// Role attributes to set. `None` leaves the attribute alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoleAttrs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentionable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionSet>,
}

impl RoleAttrs {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == RoleAttrs::default()
    }
}

/// Channel or category attributes to set. `None` leaves the attribute alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChannelAttrs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Always set on create. On update, only text ↔ announcement (research R10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<LiveChannelKind>,
    /// `""` clears the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slowmode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_limit: Option<u32>,
    /// `Some(None)` = automatic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtc_region: Option<Option<String>>,
    /// The full tag list; tags keep their ids where they already exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ForumTag>>,
    /// `Some(None)` clears it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reaction: Option<Option<String>>,
}

impl ChannelAttrs {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ChannelAttrs::default()
    }
}

/// One permission overwrite as sent to Discord, with its target possibly not created yet.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct OverwriteOp {
    pub kind: OverwriteKind,
    pub target: Ref,
    pub allow: PermissionSet,
    pub deny: PermissionSet,
}

/// One entry of a role reorder: `position` is set only when the spec gives one explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleSlot {
    pub role: Ref,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
}

/// Which list of siblings a channel reorder is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Categories, at the server level.
    Categories,
    /// Channels with no category.
    TopLevel,
    /// Channels inside one category.
    Category(Ref),
}

/// One entry of a channel reorder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelSlot {
    pub channel: Ref,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
}

/// The desired order of one list of siblings, first to last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeOrder {
    pub scope: Scope,
    pub order: Vec<ChannelSlot>,
}

/// A file to upload, with the blake3 the plan was made with (apply checks it again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileOp {
    pub asset: Asset,
    pub hash: String,
}

/// Server settings to set (`None` leaves a field alone; `Some(None)` clears it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[allow(clippy::option_option)] // three states: leave alone, clear, set
pub struct SettingsSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Option<FileOp>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<Option<FileOp>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splash: Option<Option<FileOp>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_channel: Option<Option<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_channel_flags: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules_channel: Option<Option<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_updates_channel: Option<Option<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_alerts_channel: Option<Option<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub afk_channel: Option<Option<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub afk_timeout: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_notifications: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit_content_filter: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub premium_progress_bar: Option<bool>,
    /// The whole feature list after the change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeSet<String>>,
}

/// One onboarding option to set. `id` is kept for existing options; apply makes new ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionSet {
    pub id: Option<Id>,
    pub title: String,
    pub description: Option<String>,
    pub emoji: Option<String>,
    pub roles: Vec<Ref>,
    pub channels: Vec<Ref>,
}

/// One onboarding prompt to set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptSet {
    pub id: Option<Id>,
    pub title: String,
    /// 0 multiple choice, 1 dropdown.
    pub kind: u8,
    pub single_select: bool,
    pub required: bool,
    pub in_onboarding: bool,
    pub options: Vec<OptionSet>,
}

/// The whole onboarding, replaced in one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardingSet {
    pub enabled: bool,
    /// 0 default channels only, 1 advanced.
    pub mode: u8,
    pub default_channels: Vec<Ref>,
    pub prompts: Vec<PromptSet>,
}

/// One welcome screen channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WelcomeChannelSet {
    pub channel: Ref,
    pub description: String,
    pub emoji: Option<String>,
}

/// The whole welcome screen, replaced in one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WelcomeSet {
    pub enabled: bool,
    pub description: Option<String>,
    pub channels: Vec<WelcomeChannelSet>,
}

/// An emoji, sticker or sound to upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewExpressionOp {
    pub name: String,
    pub file: FileOp,
    /// Emoji: roles allowed to use it.
    pub roles: Vec<Ref>,
    /// Stickers.
    pub description: String,
    pub tags: String,
    /// Sounds.
    pub volume: Option<Volume>,
    pub emoji: Option<String>,
}

/// Expression fields to change. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[allow(clippy::option_option)] // three states: leave alone, clear, set
pub struct ExpressionSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<Ref>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<Volume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<Option<String>>,
}

/// Where an event happens, with its channel possibly not created yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationSet {
    Voice(Ref),
    Stage(Ref),
    External(String),
}

/// Event fields to set. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[allow(clippy::option_option)] // three states: leave alone, clear, set
pub struct EventSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    /// RFC 3339, UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationSet>,
    /// Discord's `recurrence_rule`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<FileOp>,
}

/// A fixed message's body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MessageBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Value>,
    /// Discord message flags (suppress embeds, Components V2).
    pub flags: u64,
}

/// What an id written back into the spec file is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Written {
    /// Known now: a live item the spec matched without an id.
    Id(Id),
    Code(String),
    /// Made by the create op with this key; known after apply.
    New(String),
}

/// One `id = "…"` (or `code = "…"`) line to add to the spec file (research E9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteId {
    /// The item, e.g. `emoji[0]`.
    #[serde(serialize_with = "path_text")]
    pub path: SpecPath,
    /// `id`, or `code` for invites.
    pub field: &'static str,
    pub value: Written,
}

fn path_text<S: serde::Serializer>(p: &SpecPath, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(p)
}

/// What an op does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OpKind {
    CreateRole {
        key: String,
        attrs: RoleAttrs,
    },
    UpdateRole {
        id: Id,
        changes: Vec<FieldChange>,
        attrs: RoleAttrs,
    },
    /// Roles top to bottom. One request.
    ReorderRoles {
        order: Vec<RoleSlot>,
    },
    /// Covers categories too (`attrs.kind = Category`).
    CreateChannel {
        key: String,
        parent: Option<Ref>,
        attrs: ChannelAttrs,
        /// `None` = not managed: Discord's default applies.
        #[serde(skip_serializing_if = "Option::is_none")]
        overwrites: Option<Vec<OverwriteOp>>,
    },
    /// One request: fields, parent and the full overwrite list together (research R7).
    UpdateChannel {
        id: Id,
        changes: Vec<FieldChange>,
        attrs: ChannelAttrs,
        /// `Some(None)` moves the channel to the top level.
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<Option<Ref>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        overwrites: Option<Vec<OverwriteOp>>,
    },
    /// Positions only, one request (research R8).
    ReorderChannels {
        scopes: Vec<ScopeOrder>,
    },
    DeleteChannel {
        id: Id,
    },
    DeleteRole {
        id: Id,
    },
    UpdateSettings {
        changes: Vec<FieldChange>,
        set: Box<SettingsSet>,
    },
    CreateAutomod {
        key: String,
        rule: AutomodRule<Ref>,
    },
    /// Replaces the rule's settings (the trigger type can't change).
    UpdateAutomod {
        id: Id,
        changes: Vec<FieldChange>,
        rule: AutomodRule<Ref>,
    },
    DeleteAutomod {
        id: Id,
    },
    SetOnboarding {
        onboarding: OnboardingSet,
    },
    SetWelcomeScreen {
        screen: WelcomeSet,
    },
    CreateExpression {
        kind: ExpressionKind,
        key: String,
        new: Box<NewExpressionOp>,
    },
    UpdateExpression {
        kind: ExpressionKind,
        id: Id,
        changes: Vec<FieldChange>,
        set: ExpressionSet,
    },
    DeleteExpression {
        kind: ExpressionKind,
        id: Id,
    },
    /// The new webhook's URL is a secret, reported once by apply (research E8).
    CreateWebhook {
        key: String,
        name: String,
        channel: Ref,
        #[serde(skip_serializing_if = "Option::is_none")]
        avatar: Option<FileOp>,
    },
    UpdateWebhook {
        id: Id,
        changes: Vec<FieldChange>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        channel: Option<Ref>,
        #[serde(skip_serializing_if = "Option::is_none")]
        avatar: Option<FileOp>,
    },
    DeleteWebhook {
        id: Id,
    },
    CreateInvite {
        key: String,
        channel: Ref,
        max_age: u32,
        max_uses: u32,
        temporary: bool,
        roles: Vec<Ref>,
    },
    DeleteInvite {
        code: String,
    },
    CreateEvent {
        key: String,
        set: Box<EventSet>,
    },
    UpdateEvent {
        id: Id,
        changes: Vec<FieldChange>,
        set: Box<EventSet>,
    },
    DeleteEvent {
        id: Id,
    },
    PostMessage {
        key: String,
        channel: Ref,
        body: MessageBody,
        pin: bool,
    },
    EditMessage {
        channel: Id,
        id: Id,
        changes: Vec<FieldChange>,
        body: MessageBody,
    },
    SetPinned {
        channel: Id,
        id: Id,
        pinned: bool,
    },
    DeleteMessage {
        channel: Id,
        id: Id,
    },
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Op {
    /// Shown to people, e.g. `create role @Moderator`.
    pub summary: String,
    pub risk: Risk,
    /// The item the op is about (`Ref::New` for creates).
    pub target: Ref,
    /// Shared by the delete and create of a replace (research R10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replace_group: Option<u32>,
    pub kind: OpKind,
}

impl Op {
    #[must_use]
    pub fn is_delete(&self) -> bool {
        matches!(
            self.kind,
            OpKind::DeleteChannel { .. }
                | OpKind::DeleteRole { .. }
                | OpKind::DeleteAutomod { .. }
                | OpKind::DeleteExpression { .. }
                | OpKind::DeleteWebhook { .. }
                | OpKind::DeleteInvite { .. }
                | OpKind::DeleteEvent { .. }
                | OpKind::DeleteMessage { .. }
        )
    }

    /// Every role this op's overwrites refer to.
    pub fn overwrite_targets(&self) -> impl Iterator<Item = &Ref> {
        let list: &[OverwriteOp] = match &self.kind {
            OpKind::CreateChannel { overwrites: Some(o), .. } | OpKind::UpdateChannel { overwrites: Some(o), .. } => o,
            _ => &[],
        };
        list.iter().filter(|o| o.kind == OverwriteKind::Role).map(|o| &o.target)
    }
}
