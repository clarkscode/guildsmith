//! Read-only views of a server: overview, role list and channel tree (FR-002, FR-028).
//!
//! Rows carry key fields by default; extra fields are filled only when requested.

use guildsmith_core::snapshot::{LiveChannel, LiveChannelKind, Snapshot};
use guildsmith_core::{Id, PermissionSet};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::error::ServiceError;
use crate::ports::DiscordPort;
use crate::services::guilds;

/// Resolves the server input and fetches its snapshot.
pub async fn snapshot<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<Snapshot, ServiceError> {
    let id = guilds::resolve(app, guild).await?;
    Ok(app.snapshot(id).await?)
}

fn check_fields(requested: &[String], valid: &[&str]) -> Result<(), ServiceError> {
    match requested.iter().find(|f| !valid.contains(&f.as_str())) {
        Some(bad) => Err(ServiceError::Invalid(format!("unknown field `{bad}`; valid fields: {}", valid.join(", ")))),
        None => Ok(()),
    }
}

fn wants(fields: &[String], name: &str) -> bool {
    fields.iter().any(|f| f == name)
}

/// The kind of a live channel as shown in tool output.
#[must_use]
pub fn kind_name(k: LiveChannelKind) -> String {
    match k {
        LiveChannelKind::Category => "category".to_owned(),
        LiveChannelKind::Known(k) => k.as_str().to_owned(),
        LiveChannelKind::Other(n) => format!("other:{n}"),
    }
}

/// One role. `id`, `name`, `position` and `managed` are always present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RoleRow {
    pub id: Id,
    pub name: String,
    pub position: i64,
    pub managed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentionable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<String>>")]
    pub permissions: Option<PermissionSet>,
}

pub const ROLE_FIELDS: &[&str] = &["color", "hoist", "mentionable", "permissions"];

/// Roles, highest first. `fields` adds any of `ROLE_FIELDS`.
pub fn roles(s: &Snapshot, fields: &[String]) -> Result<Vec<RoleRow>, ServiceError> {
    check_fields(fields, ROLE_FIELDS)?;
    let mut list: Vec<_> = s.roles.iter().collect();
    list.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    Ok(list
        .into_iter()
        .map(|r| RoleRow {
            id: r.id,
            name: if r.id == s.guild.id { "@everyone".to_owned() } else { r.name.clone() },
            position: r.position,
            managed: r.managed,
            color: wants(fields, "color").then(|| format!("#{:06X}", r.color)),
            hoist: wants(fields, "hoist").then_some(r.hoist),
            mentionable: wants(fields, "mentionable").then_some(r.mentionable),
            permissions: wants(fields, "permissions").then_some(r.permissions),
        })
        .collect())
}

/// One node of the channel tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TreeNode {
    pub id: Id,
    pub name: String,
    /// `category`, `text`, `announcement`, `voice`, `stage`, `forum`, `media` or `other:<n>`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

pub const TREE_FIELDS: &[&str] = &["position", "topic", "nsfw"];

fn node(c: &LiveChannel, fields: &[String], children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        id: c.id,
        name: c.name.clone(),
        kind: kind_name(c.kind),
        position: wants(fields, "position").then_some(c.position),
        topic: if wants(fields, "topic") { c.topic.clone() } else { None },
        nsfw: wants(fields, "nsfw").then_some(c.nsfw),
        children,
    }
}

fn ordered(mut v: Vec<&LiveChannel>) -> Vec<&LiveChannel> {
    v.sort_by(|a, b| a.position.cmp(&b.position).then(a.id.cmp(&b.id)));
    v
}

/// Categories with their channels, then top-level channels, in Discord's order.
pub fn tree(s: &Snapshot, fields: &[String]) -> Result<Vec<TreeNode>, ServiceError> {
    check_fields(fields, TREE_FIELDS)?;
    let is_cat = |id: Option<Id>| id.and_then(|i| s.channel(i)).is_some_and(|c| c.kind == LiveChannelKind::Category);
    let mut out = Vec::new();
    for cat in ordered(s.channels.iter().filter(|c| c.kind == LiveChannelKind::Category).collect()) {
        let kids = ordered(s.channels.iter().filter(|c| c.parent_id == Some(cat.id)).collect());
        out.push(node(cat, fields, kids.into_iter().map(|k| node(k, fields, Vec::new())).collect()));
    }
    for c in
        ordered(s.channels.iter().filter(|c| c.kind != LiveChannelKind::Category && !is_cat(c.parent_id)).collect())
    {
        out.push(node(c, fields, Vec::new()));
    }
    Ok(out)
}

/// Server facts for the overview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GuildFacts {
    pub id: Id,
    pub name: String,
    pub features: Vec<String>,
    /// `false` when the bot isn't an administrator: hidden channels may be missing.
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Counts {
    pub roles: usize,
    pub categories: usize,
    pub channels: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RoleBrief {
    pub id: Id,
    pub name: String,
}

/// One compact summary of a server (`guild_overview`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Overview {
    pub guild: GuildFacts,
    pub counts: Counts,
    pub roles: Vec<RoleBrief>,
    pub tree: Vec<TreeNode>,
}

pub async fn overview<P: DiscordPort>(app: &App<P>, guild: Option<&str>) -> Result<Overview, ServiceError> {
    let s = snapshot(app, guild).await?;
    let categories = s.channels.iter().filter(|c| c.kind == LiveChannelKind::Category).count();
    Ok(Overview {
        guild: GuildFacts {
            id: s.guild.id,
            name: s.guild.name.clone(),
            features: s.guild.features.iter().cloned().collect(),
            complete: s.completeness == guildsmith_core::snapshot::Completeness::Complete,
        },
        counts: Counts { roles: s.roles.len(), categories, channels: s.channels.len() - categories },
        roles: roles(&s, &[])?.into_iter().map(|r| RoleBrief { id: r.id, name: r.name }).collect(),
        tree: tree(&s, &[])?,
    })
}
