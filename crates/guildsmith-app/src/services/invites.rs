//! The `invites` group (US3, FR-026): create invites (with roles and target users), list them
//! with uses and expiry, and revoke them.

use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;
use crate::confirm::{self, Outcome};
use crate::date;
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::{DiscordPort, InviteInfo, NewInvite};
use crate::preflight;
use crate::resolve;
use crate::services::live::{Deleted, find_member, invalid, refuse_if, resolved, role_name, server, writable};

pub const MAX_AGE: u32 = 604_800;
pub const MAX_USES: u32 = 100;
/// Target users per request.
pub const MAX_TARGETS: usize = 1000;

/// An invite as tools return it. `inviter` is a user name: untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InviteRow {
    pub code: String,
    pub url: String,
    pub channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inviter: Option<String>,
    /// Seconds; 0 = never expires.
    pub max_age: u32,
    /// 0 = unlimited.
    pub max_uses: u32,
    pub uses: u32,
    pub temporary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// `active`, `expired` or `used up`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

fn row(s: &Snapshot, i: &InviteInfo, now: i64) -> InviteRow {
    let expired = i.expires_at.as_deref().and_then(date::parse_rfc3339).is_some_and(|t| t <= now);
    let used_up = i.max_uses > 0 && i.uses >= i.max_uses;
    InviteRow {
        code: i.code.clone(),
        url: format!("https://discord.gg/{}", i.code),
        channel: s.channel(i.channel_id).map_or_else(|| i.channel_id.to_string(), |c| format!("#{}", c.name)),
        inviter: i.inviter.clone(),
        max_age: i.max_age,
        max_uses: i.max_uses,
        uses: i.uses,
        temporary: i.temporary,
        expires_at: i.expires_at.clone(),
        state: if expired {
            "expired"
        } else if used_up {
            "used up"
        } else {
            "active"
        }
        .to_owned(),
        roles: i.roles.iter().map(|r| role_name(s, *r)).collect(),
    }
}

/// An invite to create.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InviteInput {
    /// Seconds (≤ 604800); 0 = never expires. Default 86400.
    pub max_age: Option<u32>,
    /// ≤ 100; 0 = unlimited.
    pub max_uses: Option<u32>,
    pub temporary: bool,
    /// Roles new members get (below the bot's highest role).
    pub roles: Vec<String>,
    /// Only these users can use it.
    pub only_users: Vec<String>,
    pub unique: bool,
}

/// Creates an invite to a channel and returns its link.
pub async fn invite_create<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: &str,
    input: &InviteInput,
    reason: Option<&str>,
) -> Result<InviteRow, ServiceError> {
    let max_age = input.max_age.unwrap_or(86_400);
    let max_uses = input.max_uses.unwrap_or(0);
    if max_age > MAX_AGE {
        return Err(invalid("max_age is at most 604800 seconds (7 days); 0 never expires"));
    }
    if max_uses > MAX_USES {
        return Err(invalid("max_uses is at most 100; 0 is unlimited"));
    }
    if input.only_users.len() > MAX_TARGETS {
        return Err(invalid("only_users takes at most 1000 users"));
    }
    let (gid, s) = writable(app, guild).await?;
    let cid = resolved(resolve::resolve_channel(&s, channel), "channel", channel)?;
    let roles = input
        .roles
        .iter()
        .map(|r| resolved(resolve::resolve_role(&s, r), "role", r))
        .collect::<Result<Vec<Id>, _>>()?;
    let mut problems = preflight::check_tools(&s, ["invite_create"]);
    for r in &roles {
        if *r == s.guild.id || s.role(*r).is_some_and(|x| x.managed) {
            return Err(invalid(format!("{} can't be given by an invite", role_name(&s, *r))));
        }
        problems.extend(preflight::check_tools(&s, ["role_upsert"]));
        problems.extend(preflight::check_role_edit(&s, *r));
    }
    let mut targets = Vec::new();
    for u in &input.only_users {
        targets.push(match u.trim().trim_start_matches("<@").trim_end_matches('>').parse::<Id>() {
            Ok(id) => id,
            Err(_) => find_member(app, gid, u).await?.id,
        });
    }
    if !targets.is_empty() {
        problems.extend(preflight::check_tools(&s, ["invite_target_users"]));
    }
    problems.dedup();
    refuse_if(&problems)?;
    let inv = NewInvite { max_age, max_uses, temporary: input.temporary, unique: input.unique, roles };
    let name = s.channel(cid).map_or("", |c| c.name.as_str());
    let info = app.port.create_invite(cid, &inv, &audit_reason(&format!("create invite to #{name}"), reason)).await?;
    if !targets.is_empty() {
        app.port.add_invite_target_users(&info.code, &targets).await?;
    }
    Ok(row(&s, &info, date::now_secs(app.clock.as_ref())))
}

/// Every invite (or one channel's), with uses and expiry; expired and used-up invites are shown
/// with their state.
pub async fn invites_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    channel: Option<&str>,
) -> Result<Vec<InviteRow>, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    let cid = channel.map(|c| resolved(resolve::resolve_channel(&s, c), "channel", c)).transpose()?;
    refuse_if(&preflight::check_tools(&s, ["invites_list"]))?;
    let now = date::now_secs(app.clock.as_ref());
    let all = app.port.guild_invites(gid).await?;
    Ok(all.iter().filter(|i| cid.is_none_or(|c| i.channel_id == c)).map(|i| row(&s, i, now)).collect())
}

fn code(input: &str) -> String {
    input.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_owned()
}

/// Revokes an invite (a code or link) after confirmation.
pub async fn invite_revoke<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    invite: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let c = code(invite);
    let found =
        app.port.guild_invites(gid).await?.into_iter().find(|i| i.code == c).ok_or_else(|| ServiceError::NotFound {
            what: "invite",
            input: invite.to_owned(),
            suggestions: Vec::new(),
        })?;
    let what = format!("invite {c} to {} ({} use(s) so far)", row(&s, &found, 0).channel, found.uses);
    confirm::guarded(confirmed, vec![format!("revoke {what}; its link stops working")], || async {
        refuse_if(&preflight::check_tools(&s, ["invite_revoke"]))?;
        app.port.delete_invite(&c, &audit_reason(&format!("revoke invite {c}"), reason)).await?;
        Ok(Deleted { deleted: format!("invite {c}") })
    })
    .await
}

/// What `invite_target_users` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetAction {
    List,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InviteTargets {
    pub code: String,
    /// Empty: anyone can use the invite.
    pub users: Vec<Id>,
}

/// Lists, adds or removes the users an invite is limited to (≤ 1000 per request).
pub async fn invite_target_users<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    invite: &str,
    action: TargetAction,
    users: &[String],
) -> Result<InviteTargets, ServiceError> {
    if users.len() > MAX_TARGETS {
        return Err(invalid("at most 1000 users per request"));
    }
    let (gid, s) = if action == TargetAction::List { server(app, guild).await? } else { writable(app, guild).await? };
    let c = code(invite);
    if !app.port.guild_invites(gid).await?.iter().any(|i| i.code == c) {
        return Err(ServiceError::NotFound { what: "invite", input: invite.to_owned(), suggestions: Vec::new() });
    }
    if action != TargetAction::List {
        if users.is_empty() {
            return Err(invalid("give `users` to add or remove"));
        }
        refuse_if(&preflight::check_tools(&s, ["invite_target_users"]))?;
        let mut ids = Vec::new();
        for u in users {
            ids.push(match u.trim().trim_start_matches("<@").trim_end_matches('>').parse::<Id>() {
                Ok(id) => id,
                Err(_) => find_member(app, gid, u).await?.id,
            });
        }
        if action == TargetAction::Add {
            app.port.add_invite_target_users(&c, &ids).await?;
        } else {
            app.port.remove_invite_target_users(&c, &ids).await?;
        }
    }
    Ok(InviteTargets { users: app.port.invite_target_users(&c).await?, code: c })
}
