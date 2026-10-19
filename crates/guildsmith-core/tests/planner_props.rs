//! T050: planner property tests, one per data-model §3 invariant, using the pure in-memory
//! apply (`planner::simulate::simulate`).

mod common;

use std::collections::BTreeSet;

use guildsmith_core::export::to_spec;
use guildsmith_core::planner::simulate::simulate;
use guildsmith_core::planner::{Op, OpKind, Ref};
use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::spec::{
    CategorySpec, ChannelKind, ChannelSpec, Color, GuildSpec, OverwriteSpec, PruneSpec, RoleSpec, Target,
};
use guildsmith_core::{Permission, PermissionSet, diff};
use proptest::prelude::*;

/// A tiny deterministic generator, so one seed describes a whole spec.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// A random subset of `pool`, in random order.
    fn subset<'a>(&mut self, pool: &[&'a str], max: usize) -> Vec<&'a str> {
        let mut v: Vec<&str> = pool.to_vec();
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
        v.truncate(self.below(max + 1));
        v
    }

    fn perms(&mut self) -> PermissionSet {
        const POOL: &[Permission] = &[
            Permission::ViewChannel,
            Permission::SendMessages,
            Permission::ManageMessages,
            Permission::Connect,
            Permission::KickMembers,
            Permission::AttachFiles,
        ];
        POOL.iter().copied().filter(|_| self.chance(40)).collect()
    }
}

const ROLES: &[&str] = &["Admin", "Mod", "Member", "Guest", "Bot Helper"];
const CATEGORIES: &[&str] = &["Info", "Community", "Voice"];
const CHANNELS: &[&str] = &["general", "random", "news", "voice-chat", "help"];
const KINDS: &[ChannelKind] =
    &[ChannelKind::Text, ChannelKind::Voice, ChannelKind::Forum, ChannelKind::Announcement, ChannelKind::Stage];

fn overwrites(g: &mut Gen, roles: &[&str]) -> Vec<OverwriteSpec> {
    let mut targets = vec![Target::Everyone];
    targets.extend(roles.iter().map(|r| Target::RoleName((*r).to_owned())));
    let mut out = Vec::new();
    for t in targets {
        if g.chance(35) {
            let allow = g.perms();
            let deny = g.perms().difference(allow);
            out.push(OverwriteSpec { target: t, allow, deny });
        }
    }
    out
}

fn channel(g: &mut Gen, name: &str, roles: &[&str], in_category: bool) -> ChannelSpec {
    let kind = *g.pick(KINDS);
    let mut ch = ChannelSpec { name: name.to_owned(), kind, replace: true, ..ChannelSpec::default() };
    if !kind.is_voice_like() && g.chance(50) {
        ch.topic = Some(if g.chance(30) { String::new() } else { format!("about {name}") });
    }
    if matches!(kind, ChannelKind::Text | ChannelKind::Forum) && g.chance(40) {
        ch.slowmode = Some(u32::try_from(g.below(3)).unwrap() * 10);
    }
    if g.chance(30) {
        ch.nsfw = Some(g.chance(50));
    }
    if kind.is_voice_like() && g.chance(50) {
        ch.bitrate = Some(64_000);
        ch.user_limit = Some(u32::try_from(g.below(20)).unwrap());
        ch.rtc_region = Some((*g.pick(&["auto", "us-west"])).to_owned());
    }
    if kind.is_forum_like() && g.chance(50) {
        ch.tags = Some(
            g.subset(&["question", "solved", "idea"], 3)
                .into_iter()
                .map(|n| guildsmith_core::spec::ForumTag {
                    name: n.to_owned(),
                    moderated: Some(n == "solved"),
                    ..Default::default()
                })
                .collect(),
        );
    }
    if in_category && g.chance(25) {
        ch.sync_permissions = true;
    } else if g.chance(50) {
        ch.overwrites = Some(overwrites(g, roles));
    }
    ch
}

/// A random, valid spec. Every channel allows replace, so kind changes plan instead of failing.
fn spec(seed: u64) -> GuildSpec {
    let mut g = Gen(seed | 1);
    let roles = g.subset(ROLES, ROLES.len());
    let mut role_specs: Vec<RoleSpec> = roles
        .iter()
        .map(|n| {
            let mut r = RoleSpec { name: (*n).to_owned(), ..RoleSpec::default() };
            if g.chance(50) {
                r.color = Some(Color(u32::try_from(g.below(0xFF_FFFF)).unwrap()));
            }
            if g.chance(50) {
                r.hoist = Some(g.chance(50));
            }
            if g.chance(70) {
                r.permissions = Some(g.perms());
            }
            r
        })
        .collect();
    if g.chance(50) {
        role_specs.push(RoleSpec { name: "@everyone".to_owned(), permissions: Some(g.perms()), ..RoleSpec::default() });
    }
    let categories = g
        .subset(CATEGORIES, CATEGORIES.len())
        .into_iter()
        .map(|c| CategorySpec {
            name: c.to_owned(),
            overwrites: g.chance(50).then(|| overwrites(&mut g, &roles)),
            channels: g.subset(CHANNELS, 3).into_iter().map(|n| channel(&mut g, n, &roles, true)).collect(),
            ..CategorySpec::default()
        })
        .collect();
    let channels = g.subset(CHANNELS, 3).into_iter().map(|n| channel(&mut g, n, &roles, false)).collect();
    let prune = PruneSpec { roles: g.chance(50), categories: g.chance(50), channels: g.chance(50) };
    GuildSpec { roles: role_specs, categories, channels, prune, ..GuildSpec::default() }
}

/// An empty server: only `@everyone`. The bot owns it, so nothing is above it.
fn empty() -> Snapshot {
    let mut s = common::lab();
    s.roles.retain(|r| r.id == common::GUILD);
    s.channels.clear();
    s.bot.user_id = s.guild.owner_id;
    s.bot.role_ids.clear();
    s
}

/// A server built by applying a random spec to an empty one.
fn live(seed: u64) -> Snapshot {
    let base = empty();
    let mut sp = spec(seed);
    sp.prune = PruneSpec::default();
    let plan = diff(&sp, &base).expect("plans against an empty server");
    simulate(&base, &plan.ops).expect("simulated apply")
}

fn created_key(op: &Op) -> Option<&str> {
    match &op.kind {
        OpKind::CreateRole { key, .. } | OpKind::CreateChannel { key, .. } => Some(key),
        _ => None,
    }
}

fn check_order(ops: &[Op]) -> Result<(), TestCaseError> {
    let mut created: BTreeSet<&str> = BTreeSet::new();
    let mut seen_delete = false;
    for (i, op) in ops.iter().enumerate() {
        // 1. Every CreateRole comes before any op whose overwrites refer to that role.
        for t in op.overwrite_targets() {
            if let Ref::New(k) = t {
                prop_assert!(created.contains(k.as_str()), "op {i} `{}` refers to {k} before it's created", op.summary);
            }
        }
        // 2. A category's create comes before its children's creates.
        if let OpKind::CreateChannel { parent: Some(Ref::New(k)), .. }
        | OpKind::UpdateChannel { parent: Some(Some(Ref::New(k))), .. } = &op.kind
        {
            prop_assert!(created.contains(k.as_str()), "op {i} `{}` uses parent {k} before it exists", op.summary);
        }
        // 4. Every delete comes after every non-delete op.
        if op.is_delete() {
            seen_delete = true;
        } else {
            prop_assert!(!seen_delete, "op {i} `{}` runs after a delete", op.summary);
        }
        if let Some(k) = created_key(op) {
            created.insert(k);
        }
    }
    // 3. At most one ReorderRoles and one ReorderChannels: the role reorder after every role
    // create and update, the channel reorder after every create and update.
    let at =
        |f: fn(&OpKind) -> bool| ops.iter().enumerate().filter(|(_, o)| f(&o.kind)).map(|(i, _)| i).collect::<Vec<_>>();
    let role_reorders = at(|k| matches!(k, OpKind::ReorderRoles { .. }));
    let channel_reorders = at(|k| matches!(k, OpKind::ReorderChannels { .. }));
    prop_assert!(role_reorders.len() <= 1 && channel_reorders.len() <= 1);
    let role_writes = at(|k| matches!(k, OpKind::CreateRole { .. } | OpKind::UpdateRole { .. }));
    let all_writes = at(|k| {
        matches!(
            k,
            OpKind::CreateRole { .. }
                | OpKind::UpdateRole { .. }
                | OpKind::CreateChannel { .. }
                | OpKind::UpdateChannel { .. }
        )
    });
    if let Some(r) = role_reorders.first() {
        prop_assert!(role_writes.iter().all(|w| w < r));
    }
    if let Some(r) = channel_reorders.first() {
        prop_assert!(all_writes.iter().all(|w| w < r));
    }
    Ok(())
}

/// An export of `snapshot` with random edits: renames, moves, reorders, removals and prune.
fn edited_export(snapshot: &Snapshot, seed: u64) -> GuildSpec {
    let mut g = Gen(seed | 1);
    let (mut sp, _) = to_spec(snapshot);
    if g.chance(50) {
        sp.roles.reverse();
    }
    let named: Vec<usize> = (0..sp.roles.len()).filter(|i| sp.roles[*i].name != "@everyone").collect();
    if !named.is_empty() && g.chance(50) {
        let i = *g.pick(&named);
        sp.roles[i].name.push_str(" II");
    }
    if !named.is_empty() && g.chance(30) {
        let i = *g.pick(&named);
        sp.roles.remove(i);
    }
    for c in &mut sp.categories {
        if g.chance(40) {
            c.channels.reverse();
        }
        if g.chance(20) {
            c.name.push('!');
        }
        for ch in &mut c.channels {
            if g.chance(20) && !ch.kind.is_voice_like() {
                ch.topic = Some("edited".to_owned());
            }
            if g.chance(10) {
                ch.name = format!("{}-2", ch.name);
            }
        }
    }
    // Move one channel to another category or to the top level.
    if sp.categories.len() > 1 && g.chance(50) {
        let from = g.below(sp.categories.len());
        if !sp.categories[from].channels.is_empty() {
            let mut ch = sp.categories[from].channels.remove(0);
            ch.sync_permissions = false;
            let to = g.below(sp.categories.len() + 1);
            let taken = |list: &[ChannelSpec]| list.iter().any(|c| c.name == ch.name);
            if to == sp.categories.len() {
                if !taken(&sp.channels) {
                    sp.channels.push(ch);
                }
            } else if !taken(&sp.categories[to].channels) {
                sp.categories[to].channels.insert(0, ch);
            }
        }
    }
    if !sp.categories.is_empty() && g.chance(25) {
        let i = g.below(sp.categories.len());
        sp.categories.remove(i);
    }
    // Overwrites that named a role the edits renamed or removed would no longer validate.
    let role_names: BTreeSet<String> = sp.roles.iter().map(|r| r.name.clone()).collect();
    let keep = |o: &OverwriteSpec| !matches!(&o.target, Target::RoleName(n) if !role_names.contains(n));
    for c in &mut sp.categories {
        if let Some(list) = &mut c.overwrites {
            list.retain(keep);
        }
        for ch in &mut c.channels {
            if let Some(list) = &mut ch.overwrites {
                list.retain(keep);
            }
        }
    }
    for ch in &mut sp.channels {
        if let Some(list) = &mut ch.overwrites {
            list.retain(keep);
        }
    }
    sp.prune = PruneSpec { roles: g.chance(50), categories: g.chance(50), channels: g.chance(50) };
    sp
}

proptest! {
    #[test]
    fn plans_converge_and_keep_their_order(live_seed in any::<u64>(), spec_seed in any::<u64>()) {
        let snapshot = live(live_seed);
        let wanted = spec(spec_seed);
        let plan = diff(&wanted, &snapshot).map_err(|e| TestCaseError::fail(e.to_string()))?;
        check_order(&plan.ops)?;

        // Deterministic: same input, same ops and hash.
        let again = diff(&wanted, &snapshot).unwrap();
        prop_assert_eq!(&plan, &again);

        // 5. diff(spec, apply(spec, snapshot)) is empty.
        let after = simulate(&snapshot, &plan.ops).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let replan = diff(&wanted, &after).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let left: Vec<&str> = replan.ops.iter().map(|o| o.summary.as_str()).collect();
        prop_assert!(replan.ops.is_empty(), "not converged; left: {left:?}\nfirst plan: {:?}",
            plan.ops.iter().map(|o| &o.summary).collect::<Vec<_>>());
    }

    #[test]
    fn edited_exports_converge(live_seed in any::<u64>(), edit_seed in any::<u64>()) {
        let snapshot = live(live_seed);
        let wanted = edited_export(&snapshot, edit_seed);
        let plan = diff(&wanted, &snapshot).map_err(|e| TestCaseError::fail(e.to_string()))?;
        check_order(&plan.ops)?;
        let after = simulate(&snapshot, &plan.ops).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let replan = diff(&wanted, &after).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let left: Vec<&str> = replan.ops.iter().map(|o| o.summary.as_str()).collect();
        prop_assert!(replan.ops.is_empty(), "not converged; left: {left:?}\nfirst plan: {:?}",
            plan.ops.iter().map(|o| &o.summary).collect::<Vec<_>>());
    }

    #[test]
    fn exports_plan_to_nothing(live_seed in any::<u64>()) {
        let snapshot = live(live_seed);
        let (exported, _) = to_spec(&snapshot);
        let plan = diff(&exported, &snapshot).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let left: Vec<&str> = plan.ops.iter().map(|o| o.summary.as_str()).collect();
        prop_assert!(plan.ops.is_empty(), "export doesn't round-trip: {left:?}");
    }
}

#[test]
fn ops_are_grouped_in_r8_order() {
    // Roles, role order, categories, channels, channel order, then deletes.
    let snapshot = live(7);
    let wanted = spec(11);
    let plan = diff(&wanted, &snapshot).unwrap();
    let rank = |op: &Op| match &op.kind {
        OpKind::CreateRole { .. } | OpKind::UpdateRole { .. } => 0,
        OpKind::ReorderRoles { .. } => 1,
        OpKind::CreateChannel { .. } | OpKind::UpdateChannel { .. } => 2,
        OpKind::ReorderChannels { .. } => 4,
        OpKind::DeleteChannel { .. } => 5,
        OpKind::DeleteRole { .. } => 7,
    };
    let ranks: Vec<i32> = plan.ops.iter().map(rank).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    assert_eq!(ranks, sorted, "{:?}", plan.ops.iter().map(|o| &o.summary).collect::<Vec<_>>());
}
