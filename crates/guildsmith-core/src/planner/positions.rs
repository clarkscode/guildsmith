//! Turning a desired order into concrete positions.
//!
//! guildsmith only permutes the slots its managed items already occupy, so unmanaged items keep
//! their places. The apply service computes positions on a fresh snapshot right before a reorder
//! request. The planner only asks whether items are already in order, comparing relative order
//! with ties broken by id: Discord servers often have equal positions (text and voice channels
//! are numbered separately), and an unchanged export must plan to nothing (SC-001).

use crate::id::Id;
use crate::snapshot::Snapshot;

/// Sorts `slots` ascending and makes them strictly increasing, so ties can't hide the order.
fn distinct_ascending<T: Copy + Ord + std::ops::Add<Output = T> + From<u8>>(mut slots: Vec<T>) -> Vec<T> {
    slots.sort_unstable();
    for i in 1..slots.len() {
        if slots[i] <= slots[i - 1] {
            slots[i] = slots[i - 1] + T::from(1);
        }
    }
    slots
}

/// Positions for roles listed **top to bottom**. Explicit positions are used as given. Returns
/// only the roles whose position changes; empty means the roles are already in order.
#[must_use]
pub fn role_positions(snapshot: &Snapshot, order: &[(Id, Option<i64>)]) -> Vec<(Id, i64)> {
    let current = |id: Id| snapshot.role(id).map_or(0, |r| r.position);
    let free: Vec<Id> = order.iter().filter(|(_, p)| p.is_none()).map(|(id, _)| *id).collect();
    let slots = distinct_ascending(free.iter().map(|id| current(*id)).collect());
    // Top to bottom gets the highest slot first.
    let mut wanted: Vec<(Id, i64)> = free.iter().rev().copied().zip(slots).collect();
    wanted.extend(order.iter().filter_map(|(id, p)| p.map(|p| (*id, p))));
    wanted.retain(|(id, p)| current(*id) != *p);
    wanted.sort_unstable();
    wanted
}

/// Positions for channels or categories listed **first to last** within one parent. Explicit
/// positions are used as given. Returns only the channels whose position changes.
#[must_use]
pub fn channel_positions(snapshot: &Snapshot, order: &[(Id, Option<i32>)]) -> Vec<(Id, i32)> {
    let current = |id: Id| snapshot.channel(id).map_or(0, |c| c.position);
    let free: Vec<Id> = order.iter().filter(|(_, p)| p.is_none()).map(|(id, _)| *id).collect();
    let slots = distinct_ascending(free.iter().map(|id| current(*id)).collect());
    let mut wanted: Vec<(Id, i32)> = free.iter().copied().zip(slots).collect();
    wanted.extend(order.iter().filter_map(|(id, p)| p.map(|p| (*id, p))));
    wanted.retain(|(id, p)| current(*id) != *p);
    wanted.sort_unstable();
    wanted
}

/// Whether roles listed **top to bottom** already sit in that order (ties broken by id, as
/// export lists them) and every explicit position holds.
#[must_use]
pub fn roles_in_order(snapshot: &Snapshot, order: &[(Id, Option<i64>)]) -> bool {
    let current = |id: Id| snapshot.role(id).map_or(0, |r| r.position);
    let explicit_ok = order.iter().all(|(id, p)| p.is_none_or(|p| current(*id) == p));
    let free: Vec<(i64, Id)> = order.iter().filter(|(_, p)| p.is_none()).map(|(id, _)| (current(*id), *id)).collect();
    explicit_ok && free.windows(2).all(|w| w[0].0 > w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1))
}

/// Whether channels listed **first to last** already sit in that order (ties broken by id, as
/// export lists them) and every explicit position holds.
#[must_use]
pub fn channels_in_order(snapshot: &Snapshot, order: &[(Id, Option<i32>)]) -> bool {
    let current = |id: Id| snapshot.channel(id).map_or(0, |c| c.position);
    let explicit_ok = order.iter().all(|(id, p)| p.is_none_or(|p| current(*id) == p));
    let free: Vec<(i32, Id)> = order.iter().filter(|(_, p)| p.is_none()).map(|(id, _)| (current(*id), *id)).collect();
    explicit_ok && free.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1))
}
