//! `[[events]]`: matched by id, then name. Scheduled events are managed fully; an active
//! event changes only its description and end; completed, canceled and past events are
//! reported and never made again (data-model §4).

use std::collections::BTreeSet;

use super::{Refs, SpecAssets, change, changed, spec_hash, summary};
use crate::live::{EventInfo, EventLocation, LiveExtras, asset_key};
use crate::planner::Builder;
use crate::planner::ops::{EventSet, FieldChange, FileOp, LocationSet, OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::events::{EventSpec, LocationSpec, recurrence_rule, same_rule};
use crate::time;

fn key(name: &str) -> String {
    format!("event:{name}")
}

fn location(b: &mut Builder<'_>, refs: &Refs, path: &SpecPath, l: &LocationSpec) -> Option<LocationSet> {
    let r = |b: &mut Builder<'_>, c: &str| match b.channel_ref(refs, c) {
        Ok(r) => Some(r),
        Err(e) => {
            b.errors.push(format!("{path}.location: {e}"));
            None
        }
    };
    Some(match l {
        LocationSpec::Voice(c) => LocationSet::Voice(r(b, c)?),
        LocationSpec::Stage(c) => LocationSet::Stage(r(b, c)?),
        LocationSpec::External { place } => LocationSet::External(place.clone()),
    })
}

fn same_location(want: &LocationSet, live: &EventLocation) -> bool {
    match (want, live) {
        (LocationSet::Voice(r), EventLocation::Voice(id)) | (LocationSet::Stage(r), EventLocation::Stage(id)) => {
            *r == Ref::Id(*id)
        }
        (LocationSet::External(a), EventLocation::External(b)) => a == b,
        _ => false,
    }
}

fn secs(rfc: Option<&str>) -> Option<i64> {
    rfc.and_then(time::parse_rfc3339)
}

/// The full set of fields for a new event.
fn full(e: &EventSpec, location: LocationSet, image: Option<FileOp>) -> EventSet {
    let start = e.start.to_utc();
    EventSet {
        name: Some(e.name.clone()),
        description: Some(e.description.clone().filter(|d| !d.is_empty())),
        recurrence: e.recurrence.as_ref().and_then(|r| recurrence_rule(r, &start).ok()),
        start: Some(start),
        end: e.end.as_ref().map(crate::spec::events::SpecTime::to_utc),
        location: Some(location),
        image,
    }
}

/// Changes to a scheduled (`active = false`) or active event.
fn edits(
    e: &EventSpec,
    want: &EventSet,
    l: &EventInfo,
    image_same: bool,
    active: bool,
    ch: &mut Vec<FieldChange>,
) -> (EventSet, Vec<&'static str>) {
    let mut set = EventSet::default();
    let mut blocked = Vec::new();
    let desc = e.description.clone().filter(|d| !d.is_empty());
    if desc != l.description {
        changed(ch, "description");
        set.description = Some(desc);
    }
    if let Some(end) = &e.end
        && Some(end.secs) != secs(l.end.as_deref())
    {
        change(ch, "end", l.end.as_deref().unwrap_or("none"), end.to_utc());
        set.end = Some(end.to_utc());
    }
    let mut other = |field: &'static str, differs: bool, ch: &mut Vec<FieldChange>| {
        if differs {
            if active {
                blocked.push(field);
            } else {
                changed(ch, field);
            }
        }
        differs && !active
    };
    if other("name", e.name != l.name, ch) {
        set.name = Some(e.name.clone());
    }
    if other("start", Some(e.start.secs) != secs(Some(&l.start)), ch) {
        set.start.clone_from(&want.start);
    }
    let loc = want.location.as_ref().is_some_and(|w| !same_location(w, &l.location));
    if other("location", loc, ch) {
        set.location.clone_from(&want.location);
    }
    let rec = match (&want.recurrence, &l.recurrence) {
        (Some(w), Some(h)) => !same_rule(w, h),
        (Some(_), None) => true,
        _ => false,
    };
    if other("recurrence", rec, ch) {
        set.recurrence.clone_from(&want.recurrence);
    }
    if other("image", !image_same, ch) {
        set.image.clone_from(&want.image);
    }
    (set, blocked)
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras, assets: &SpecAssets) {
    let Some(live) = &x.events else { return };
    let mut used = BTreeSet::new();
    for (i, e) in b.spec.events.iter().enumerate() {
        let path = SpecPath::root().key("events").index(i);
        let Some(loc) = location(b, refs, &path, &e.location) else { continue };
        let image = match &e.image {
            Some(a) => {
                let Some(h) = spec_hash(b, assets, &path.clone().key("image")).cloned() else { continue };
                Some(FileOp { asset: a.clone(), hash: h })
            }
            None => None,
        };
        let want = full(e, loc, image);
        let found = e.id.and_then(|id| live.iter().find(|l| l.id == id)).or_else(|| {
            live.iter().find(|l| {
                l.name == e.name && matches!(l.status.as_str(), "scheduled" | "active") && !used.contains(&l.id)
            })
        });
        let Some(l) = found else {
            if e.start.secs <= x.now || (e.id.is_some() && !b.foreign()) {
                b.warnings.push(format!(
                    "{path}: event {} {}; it isn't made again",
                    e.name,
                    if e.id.is_some() { "no longer exists" } else { "starts in the past" }
                ));
                continue;
            }
            b.op(
                Phase::Events,
                format!("create event {} at {}", e.name, e.start),
                Risk::Safe,
                Ref::New(key(&e.name)),
                OpKind::CreateEvent { key: key(&e.name), set: Box::new(want) },
            );
            b.write_id(path, "id", Written::New(key(&e.name)));
            continue;
        };
        used.insert(l.id);
        if e.id != Some(l.id) {
            b.write_id(path.clone(), "id", Written::Id(l.id));
        }
        if !matches!(l.status.as_str(), "scheduled" | "active") {
            b.warnings.push(format!("{path}: event {} is {}; it's left as it is", e.name, l.status));
            continue;
        }
        let image_same = want
            .image
            .as_ref()
            .is_none_or(|f| l.image.is_some() && x.asset_hashes.get(&asset_key("event", l.id)) == Some(&f.hash));
        let active = l.status == "active";
        let mut ch = Vec::new();
        let (set, blocked) = edits(e, &want, l, image_same, active, &mut ch);
        if !blocked.is_empty() {
            b.warnings.push(format!(
                "{path}: event {} has started, so only its description and end can change (not {})",
                e.name,
                blocked.join(", ")
            ));
        }
        if !ch.is_empty() {
            b.op(
                Phase::Events,
                summary(format!("update event {}", l.name), &ch),
                Risk::Safe,
                Ref::Id(l.id),
                OpKind::UpdateEvent { id: l.id, changes: ch, set: Box::new(set) },
            );
        }
    }
    for l in live.iter().filter(|l| !used.contains(&l.id) && l.status == "scheduled") {
        if b.spec.prune.events {
            b.op(
                Phase::DeleteEvents,
                format!("delete event {}", l.name),
                Risk::Destructive,
                Ref::Id(l.id),
                OpKind::DeleteEvent { id: l.id },
            );
        } else if !b.spec.events.is_empty() {
            b.unmanaged.push(format!("event {} (not in spec, prune.events = false)", l.name));
        }
    }
}
