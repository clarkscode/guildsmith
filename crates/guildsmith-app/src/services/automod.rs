//! The `automod` group (US4, FR-028): list, create, update, enable/disable and delete Discord's
//! auto-moderation rules, with every trigger, action and exemption Discord gives bots.
//!
//! Rules are checked against Discord's limits before anything is sent (research E4), and the
//! limit is named. Disabling or deleting a rule needs confirmation.

use guildsmith_core::Id;
use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::spec::automod::{self, Action, AutomodRule, Trigger};
use schemars::JsonSchema;
use serde::Serialize;

use crate::App;
use crate::confirm::{self, Outcome};
use crate::error::ServiceError;
use crate::policy::audit_reason;
use crate::ports::DiscordPort;
use crate::preflight::{self, PreflightProblem};
use crate::resolve;
use crate::services::live::{Deleted, invalid, refuse_if, resolved, role_name, server, writable};

/// A rule with names instead of ids, as tools show it.
pub type RuleView = AutomodRule<String>;

fn view(s: &Snapshot, r: &AutomodRule<Id>) -> RuleView {
    let channel = |id: Id| s.channel(id).map_or_else(|| id.to_string(), |c| format!("#{}", c.name));
    let roles: Vec<String> = r.exempt_roles.iter().map(|id| role_name(s, *id)).collect();
    let channels: Vec<String> = r.exempt_channels.iter().map(|id| channel(*id)).collect();
    let mut v = r.clone().map_refs::<String, std::convert::Infallible>(|id| Ok(channel(id))).expect("infallible");
    v.exempt_roles = roles;
    v.exempt_channels = channels;
    v
}

/// Every rule, with channel and role names.
pub async fn automod_rules_list<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
) -> Result<Vec<RuleView>, ServiceError> {
    let (gid, s) = server(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["automod_rules_list"]))?;
    Ok(app.port.automod_rules(gid).await?.iter().map(|r| view(&s, r)).collect())
}

/// The fields of a rule to create, or to change on an existing one (`None` keeps it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleInput {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub trigger: Option<Trigger>,
    pub actions: Option<Vec<Action<String>>>,
    pub exempt_roles: Option<Vec<String>>,
    pub exempt_channels: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RuleUpserted {
    pub rule: RuleView,
    pub created: bool,
    /// What changed; empty when nothing needed to.
    pub changes: Vec<String>,
}

fn find(rules: &[AutomodRule<Id>], input: &str) -> Result<AutomodRule<Id>, ServiceError> {
    let raw = input.trim();
    if let Some(r) = rules.iter().find(|r| r.id.is_some_and(|i| i.to_string() == raw)) {
        return Ok(r.clone());
    }
    let hits: Vec<&AutomodRule<Id>> = rules.iter().filter(|r| r.name == raw).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(ServiceError::NotFound {
            what: "automod rule",
            input: input.to_owned(),
            suggestions: guildsmith_core::suggest::closest(raw, rules.iter().map(|r| r.name.as_str()), 3),
        }),
        many => Err(ServiceError::Ambiguous {
            what: "automod rule",
            input: input.to_owned(),
            candidates: many
                .iter()
                .map(|r| resolve::Candidate { name: r.name.clone(), id: r.id.unwrap_or(Id(0)), parent: None })
                .collect(),
        }),
    }
}

/// Names → ids: channels for alerts and exemptions, roles for exemptions.
fn resolve_rule(s: &Snapshot, rule: AutomodRule<String>) -> Result<AutomodRule<Id>, ServiceError> {
    let roles = rule
        .exempt_roles
        .iter()
        .map(|r| resolved(resolve::resolve_role(s, r), "role", r))
        .collect::<Result<Vec<Id>, _>>()?;
    let mut out = AutomodRule { exempt_roles: Vec::new(), ..rule }
        .map_refs(|c: String| resolved(resolve::resolve_channel(s, &c), "channel", &c))?;
    out.exempt_roles = roles;
    Ok(out)
}

fn describe_changes(old: &AutomodRule<Id>, new: &AutomodRule<Id>) -> Vec<String> {
    let mut out = Vec::new();
    if old.name != new.name {
        out.push(format!("name {:?} → {:?}", old.name, new.name));
    }
    if old.enabled != new.enabled {
        out.push(format!("enabled {} → {}", old.enabled, new.enabled));
    }
    if old.trigger != new.trigger {
        out.push("trigger settings".to_owned());
    }
    if old.actions != new.actions {
        out.push("actions".to_owned());
    }
    if old.exempt_roles != new.exempt_roles {
        out.push("exempt roles".to_owned());
    }
    if old.exempt_channels != new.exempt_channels {
        out.push("exempt channels".to_owned());
    }
    out
}

/// Creates a rule (no `rule`), or changes an existing one by id or name. Only what differs is
/// sent. Turning an enabled rule off needs confirmation.
pub async fn automod_rule_upsert<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    rule: Option<&str>,
    input: &RuleInput,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<RuleUpserted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    let mut problems = preflight::check_tools(&s, ["automod_rule_upsert"]);
    refuse_if(&problems)?;
    let existing = app.port.automod_rules(gid).await?;
    let current = rule.map(|r| find(&existing, r)).transpose()?;
    let wanted: AutomodRule<String> = match &current {
        Some(c) => {
            let cv = view(&s, c);
            if let Some(t) = &input.trigger
                && t.code() != c.trigger.code()
            {
                return Err(invalid(format!(
                    "a rule's trigger type can't change ({} → {}); delete it and create a new one",
                    c.trigger.name(),
                    t.name()
                )));
            }
            AutomodRule {
                id: c.id,
                name: input.name.clone().unwrap_or(cv.name),
                enabled: input.enabled.unwrap_or(cv.enabled),
                trigger: input.trigger.clone().unwrap_or(cv.trigger),
                actions: input.actions.clone().unwrap_or(cv.actions),
                exempt_roles: input.exempt_roles.clone().unwrap_or(cv.exempt_roles),
                exempt_channels: input.exempt_channels.clone().unwrap_or(cv.exempt_channels),
            }
        }
        None => AutomodRule {
            id: None,
            name: input.name.clone().ok_or_else(|| invalid("`name` is needed to create a rule"))?,
            enabled: input.enabled.unwrap_or(true),
            trigger: input.trigger.clone().ok_or_else(|| invalid("`trigger` is needed to create a rule"))?,
            actions: input.actions.clone().ok_or_else(|| invalid("`actions` is needed to create a rule"))?,
            exempt_roles: input.exempt_roles.clone().unwrap_or_default(),
            exempt_channels: input.exempt_channels.clone().unwrap_or_default(),
        },
    };
    let mut errors = automod::problems(&wanted);
    let others = existing.iter().filter(|r| current.as_ref().is_none_or(|c| r.id != c.id)).map(|r| &r.trigger);
    errors.extend(automod::count_problems(others.chain(std::iter::once(&wanted.trigger))));
    if !errors.is_empty() {
        return Err(invalid(errors.join("; ")));
    }
    let new = resolve_rule(&s, wanted)?;
    if new.actions.iter().any(|a| matches!(a, Action::Timeout { .. })) {
        problems.extend(preflight::check_tools(&s, ["member_timeout"]).into_iter().map(|p| match p {
            PreflightProblem::MissingPermission { perm, .. } => {
                PreflightProblem::MissingPermission { perm, needed_by: "an automod timeout action".to_owned() }
            }
            other => other,
        }));
    }
    refuse_if(&problems)?;
    let Some(c) = current else {
        let created = app
            .port
            .create_automod(gid, &new, &audit_reason(&format!("create automod rule {}", new.name), reason))
            .await?;
        return Ok(Outcome::Done(RuleUpserted {
            rule: view(&s, &created),
            created: true,
            changes: vec![format!("created {}", new.name)],
        }));
    };
    let changes = describe_changes(&c, &new);
    if changes.is_empty() {
        return Ok(Outcome::Done(RuleUpserted { rule: view(&s, &c), created: false, changes }));
    }
    let id = c.id.expect("live rules have ids");
    let summary = format!("update automod rule {}: {}", c.name, changes.join(", "));
    let run = || async {
        let updated = app.port.update_automod(gid, id, &new, &audit_reason(&summary, reason)).await?;
        Ok(RuleUpserted { rule: view(&s, &updated), created: false, changes: changes.clone() })
    };
    if c.enabled && !new.enabled {
        let would = vec![format!("turn off the automod rule {} ({id}); it stops protecting the server", c.name)];
        confirm::guarded(confirmed, would, run).await
    } else {
        run().await.map(Outcome::Done)
    }
}

/// Deletes a rule after confirmation.
pub async fn automod_rule_delete<P: DiscordPort>(
    app: &App<P>,
    guild: Option<&str>,
    rule: &str,
    confirmed: bool,
    reason: Option<&str>,
) -> Result<Outcome<Deleted>, ServiceError> {
    let (gid, s) = writable(app, guild).await?;
    refuse_if(&preflight::check_tools(&s, ["automod_rule_delete"]))?;
    let r = find(&app.port.automod_rules(gid).await?, rule)?;
    let id = r.id.expect("live rules have ids");
    let what = format!("automod rule {} ({id}, {})", r.name, r.trigger.name());
    confirm::guarded(confirmed, vec![format!("delete {what}")], || async {
        app.port.delete_automod(gid, id, &audit_reason(&format!("delete {what}"), reason)).await?;
        Ok(Deleted { deleted: what.clone() })
    })
    .await
}
