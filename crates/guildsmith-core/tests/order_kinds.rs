//! T089: ops for every kind follow data-model §5: v1 structure → settings → expressions →
//! webhooks → automod → onboarding/welcome → events → invites → fixed messages and pins →
//! deletes in reverse dependency order. Ids to write back aren't ops; they come after.

mod common;

use common::kinds;
use guildsmith_core::diff_with;
use guildsmith_core::planner::simulate::simulate_with;
use guildsmith_core::planner::{Op, OpKind};

fn rank(op: &Op) -> u32 {
    match &op.kind {
        OpKind::CreateRole { .. } | OpKind::UpdateRole { .. } => 0,
        OpKind::ReorderRoles { .. } => 1,
        OpKind::CreateChannel { .. } | OpKind::UpdateChannel { .. } => 2,
        OpKind::ReorderChannels { .. } => 3,
        OpKind::UpdateSettings { .. } => 4,
        OpKind::CreateExpression { .. } | OpKind::UpdateExpression { .. } => 5,
        OpKind::CreateWebhook { .. } | OpKind::UpdateWebhook { .. } => 6,
        OpKind::CreateAutomod { .. } | OpKind::UpdateAutomod { .. } => 7,
        OpKind::SetOnboarding { .. } | OpKind::SetWelcomeScreen { .. } => 8,
        OpKind::CreateEvent { .. } | OpKind::UpdateEvent { .. } => 9,
        OpKind::CreateInvite { .. } => 10,
        OpKind::PostMessage { .. } | OpKind::EditMessage { .. } | OpKind::SetPinned { .. } => 11,
        OpKind::DeleteMessage { .. } => 12,
        OpKind::DeleteInvite { .. } => 13,
        OpKind::DeleteEvent { .. } => 14,
        OpKind::DeleteAutomod { .. } => 15,
        OpKind::DeleteWebhook { .. } => 16,
        OpKind::DeleteExpression { .. } => 17,
        OpKind::DeleteChannel { .. } => 18,
        OpKind::DeleteRole { .. } => 19,
    }
}

fn assert_ordered(ops: &[Op]) {
    let ranks: Vec<u32> = ops.iter().map(rank).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    assert_eq!(ranks, sorted, "{:#?}", ops.iter().map(|o| &o.summary).collect::<Vec<_>>());
}

#[test]
fn creates_follow_the_dependency_order() {
    let spec = kinds::parse(&kinds::full_text());
    let plan = diff_with(&spec, &common::lab(), &kinds::empty(), &kinds::assets(&spec)).unwrap();
    assert!(plan.ops.len() > 15, "{}", plan.ops.len());
    assert_ordered(&plan.ops);
}

#[test]
fn deletes_come_last_in_reverse_dependency_order() {
    let spec = kinds::parse(&kinds::full_text());
    let x0 = kinds::empty();
    let first = diff_with(&spec, &common::lab(), &x0, &kinds::assets(&spec)).unwrap();
    let (s, x) = simulate_with(&common::lab(), &x0, &first.ops).unwrap();

    // Prune everything the new spec drops, and change a few things that stay.
    let mut pruned = spec.clone();
    pruned.automod.clear();
    pruned.emoji.clear();
    pruned.webhooks.clear();
    pruned.invites.clear();
    pruned.events.clear();
    pruned.messages.clear();
    pruned.messages.push(guildsmith_core::spec::messages::FixedMessageSpec {
        content: Some("New rules".to_owned()),
        ..spec.messages[0].clone()
    });
    pruned.stickers[0].tags = "tools".to_owned();
    pruned.prune = guildsmith_core::spec::PruneSpec {
        automod: true,
        emoji: true,
        webhooks: true,
        invites: true,
        events: true,
        messages: true,
        channels: true,
        ..Default::default()
    };
    pruned.categories[0].channels.retain(|c| c.name != "mod-log");
    let plan = diff_with(&pruned, &s, &x, &kinds::assets(&pruned)).unwrap();
    for k in [
        "delete automod",
        "delete emoji",
        "delete webhook",
        "delete invite",
        "delete event",
        "delete the bot's message",
        "delete channel",
    ] {
        assert!(plan.ops.iter().any(|o| o.summary.starts_with(k)), "{k} missing: {:#?}", plan.ops);
    }
    assert_ordered(&plan.ops);
    let first_delete = plan.ops.iter().position(Op::is_delete).unwrap();
    assert!(plan.ops[first_delete..].iter().all(Op::is_delete), "deletes are last");
}
