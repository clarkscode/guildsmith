//! T018: one fixture per step of research R13; each result names the deciding step.

mod common;

use common::{BOT, GUILD, OWNER, perms};
use guildsmith_core::explain::{MemberInfo, StepKind, Who, explain};
use guildsmith_core::snapshot::OverwriteKind;
use guildsmith_core::{Id, Permission, PermissionSet};

fn member(id: u64, roles: &[u64]) -> Who {
    Who::Member(MemberInfo { id: Id(id), role_ids: roles.iter().map(|r| Id(*r)).collect(), timed_out: false })
}

const GENERAL: Id = Id(101);
const MOD_LOG: Id = Id(102);
const LOBBY: Id = Id(103);

#[test]
fn step1_owner_has_everything() {
    let s = common::lab();
    let e = explain(&s, MOD_LOG, &member(OWNER.0, &[]), Some(Permission::ViewChannel)).unwrap();
    assert_eq!(e.allowed, PermissionSet::all_known());
    assert_eq!(e.steps[e.decided_by.unwrap()].step, StepKind::Owner);
}

#[test]
fn step2_base_is_everyone_plus_roles() {
    let s = common::lab();
    let e = explain(&s, GENERAL, &member(50, &[10]), Some(Permission::KickMembers)).unwrap();
    assert!(e.allowed.contains(Permission::KickMembers));
    assert!(e.allowed.contains(Permission::SendMessages));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::Base);
    assert!(step.source.contains("@Moderator"), "{}", step.source);
}

#[test]
fn step3_administrator_skips_overwrites() {
    let mut s = common::lab();
    s.roles[2].permissions = perms(&[Permission::Administrator]);
    // #mod-log denies @everyone view; an administrator still sees it.
    let e = explain(&s, MOD_LOG, &member(50, &[11]), Some(Permission::ViewChannel)).unwrap();
    assert!(e.allowed.contains(Permission::ViewChannel));
    assert_eq!(e.steps.last().unwrap().step, StepKind::Administrator);
}

#[test]
fn step4_everyone_overwrite_denies() {
    let s = common::lab();
    let e = explain(&s, MOD_LOG, &member(50, &[11]), Some(Permission::ViewChannel)).unwrap();
    assert!(!e.allowed.contains(Permission::ViewChannel));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::EveryoneOverwrite);
    assert_eq!(step.source, "overwrite for @everyone");
    assert!(e.allowed.is_empty(), "no view_channel removes everything");
}

#[test]
fn step5_role_overwrites_allow_after_everyone_deny() {
    let s = common::lab();
    let e = explain(&s, MOD_LOG, &member(50, &[10]), Some(Permission::ViewChannel)).unwrap();
    assert!(e.allowed.contains(Permission::ViewChannel));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::RoleOverwrites);
    assert!(step.source.contains("@Moderator"));
}

#[test]
fn step5_role_overwrites_union_deny_then_allow() {
    let mut s = common::lab();
    let general = s.channels.iter_mut().find(|c| c.id == GENERAL).unwrap();
    general.overwrites = vec![
        common::overwrite(Id(10), OverwriteKind::Role, perms(&[Permission::SendMessages]), PermissionSet::EMPTY),
        common::overwrite(Id(11), OverwriteKind::Role, PermissionSet::EMPTY, perms(&[Permission::SendMessages])),
    ];
    // Combined: deny is applied first, then allow, so allow wins across role overwrites.
    let e = explain(&s, GENERAL, &member(50, &[10, 11]), Some(Permission::SendMessages)).unwrap();
    assert!(e.allowed.contains(Permission::SendMessages));
}

#[test]
fn step6_member_overwrite_wins_last() {
    let mut s = common::lab();
    let general = s.channels.iter_mut().find(|c| c.id == GENERAL).unwrap();
    general.overwrites.push(common::overwrite(
        Id(50),
        OverwriteKind::Member,
        PermissionSet::EMPTY,
        perms(&[Permission::SendMessages]),
    ));
    let e = explain(&s, GENERAL, &member(50, &[10]), Some(Permission::SendMessages)).unwrap();
    assert!(!e.allowed.contains(Permission::SendMessages));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::MemberOverwrite);
    assert_eq!(step.source, "overwrite for member:50");
}

#[test]
fn step7_no_send_messages_removes_message_permissions() {
    let mut s = common::lab();
    let general = s.channels.iter_mut().find(|c| c.id == GENERAL).unwrap();
    general.overwrites.push(common::overwrite(
        GUILD,
        OverwriteKind::Role,
        PermissionSet::EMPTY,
        perms(&[Permission::SendMessages]),
    ));
    let e = explain(&s, GENERAL, &member(50, &[11]), Some(Permission::AttachFiles)).unwrap();
    assert!(!e.allowed.contains(Permission::AttachFiles));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::Implicit);
    assert!(step.source.contains("send_messages"));
}

#[test]
fn step7_no_connect_removes_voice_permissions() {
    let mut s = common::lab();
    let lobby = s.channels.iter_mut().find(|c| c.id == LOBBY).unwrap();
    lobby.overwrites.push(common::overwrite(
        GUILD,
        OverwriteKind::Role,
        PermissionSet::EMPTY,
        perms(&[Permission::Connect]),
    ));
    let e = explain(&s, LOBBY, &member(50, &[11]), Some(Permission::Speak)).unwrap();
    assert!(!e.allowed.contains(Permission::Speak));
    assert_eq!(e.steps[e.decided_by.unwrap()].step, StepKind::Implicit);
}

#[test]
fn step7_timed_out_keeps_only_view_and_history() {
    let s = common::lab();
    let who = Who::Member(MemberInfo { id: Id(50), role_ids: vec![Id(10)], timed_out: true });
    let e = explain(&s, GENERAL, &who, Some(Permission::SendMessages)).unwrap();
    assert_eq!(e.allowed, perms(&[Permission::ViewChannel, Permission::ReadMessageHistory]));
    let step = &e.steps[e.decided_by.unwrap()];
    assert_eq!(step.step, StepKind::Implicit);
    assert!(step.source.contains("timed out"));
}

#[test]
fn role_subject_and_unknown_inputs() {
    let s = common::lab();
    let e = explain(&s, MOD_LOG, &Who::Role(Id(10)), Some(Permission::ViewChannel)).unwrap();
    assert!(e.allowed.contains(Permission::ViewChannel));
    assert!(explain(&s, Id(999), &Who::Role(Id(10)), None).is_err());
    assert!(explain(&s, GENERAL, &Who::Role(Id(999)), None).is_err());
    // The bot (id 2) is not the owner, so step 1 does not apply to it.
    let e = explain(&s, GENERAL, &member(BOT.0, &[10]), None).unwrap();
    assert!(e.steps.iter().all(|st| st.step != StepKind::Owner));
}
