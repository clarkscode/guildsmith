//! `[[messages]]`: fixed messages the bot posted. With an `id`, that message (it must be the
//! bot's); without, an exact match of content and embeds among the bot's last 100 messages in
//! the channel. Only the bot's own messages are edited or deleted (data-model §4).

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Refs, changed, summary};
use crate::id::Id;
use crate::live::{COMPONENTS_V2, LiveExtras, LiveMessage, SUPPRESS_EMBEDS};
use crate::planner::Builder;
use crate::planner::ops::{MessageBody, OpKind, Ref, Risk, Written};
use crate::planner::order::Phase;
use crate::spec::SpecPath;
use crate::spec::messages::FixedMessageSpec;

fn key(i: usize) -> String {
    format!("message:{i}")
}

fn body(m: &FixedMessageSpec) -> MessageBody {
    let mut flags = 0;
    if m.suppress_embeds == Some(true) {
        flags |= SUPPRESS_EMBEDS;
    }
    if m.components_v2.is_some() {
        flags |= COMPONENTS_V2;
    }
    MessageBody {
        content: m.content.clone().filter(|c| !c.is_empty()),
        embeds: m.embeds.clone(),
        components: m.components_v2.clone(),
        flags,
    }
}

/// Whether `have` says everything `want` says. Discord adds fields to embeds and components it
/// stores (`type`, proxy URLs, component ids), so extra fields in `have` don't count.
fn covers(want: &Value, have: &Value) -> bool {
    match (want, have) {
        (Value::Object(w), Value::Object(h)) => w.iter().all(|(k, v)| h.get(k).is_some_and(|x| covers(v, x))),
        (Value::Array(w), Value::Array(h)) => w.len() == h.len() && w.iter().zip(h).all(|(a, b)| covers(a, b)),
        _ => want == have,
    }
}

fn same_embeds(want: &[Value], have: &[Value]) -> bool {
    want.len() == have.len() && want.iter().zip(have).all(|(a, b)| covers(a, b))
}

fn same_components(want: Option<&Value>, have: Option<&Value>) -> bool {
    match (want, have) {
        (Some(w), Some(h)) => covers(w, h),
        (None, None) => true,
        _ => false,
    }
}

fn same_content(want: &MessageBody, l: &LiveMessage) -> bool {
    want.content.as_deref().unwrap_or("") == l.content
        && same_embeds(&want.embeds, &l.embeds)
        && same_components(want.components.as_ref(), l.components.as_ref())
}

fn label(m: &FixedMessageSpec, channel: &Ref) -> String {
    let first = m.content.as_deref().and_then(|c| c.lines().next()).unwrap_or("");
    let short: String = first.chars().take(30).collect();
    if short.is_empty() { format!("message in {channel}") } else { format!("message in {channel} (\"{short}…\")") }
}

pub(super) fn plan(b: &mut Builder<'_>, refs: &Refs, x: &LiveExtras) {
    let Some(by_channel) = &x.messages else { return };
    let mut used: BTreeSet<Id> = BTreeSet::new();
    let mut listed: BTreeSet<Id> = BTreeSet::new();
    for (i, m) in b.spec.messages.iter().enumerate() {
        let path = SpecPath::root().key("messages").index(i);
        let channel = match b.channel_ref(refs, &m.channel) {
            Ok(c) => c,
            Err(e) => {
                b.errors.push(format!("{path}.channel: {e}"));
                continue;
            }
        };
        let want = body(m);
        let post = |b: &mut Builder<'_>, why: &str| {
            b.op(
                Phase::Messages,
                format!("post {}{why}", label(m, &channel)),
                Risk::Safe,
                Ref::New(key(i)),
                OpKind::PostMessage { key: key(i), channel: channel.clone(), body: want.clone(), pin: m.pinned },
            );
            b.write_id(path.clone(), "id", Written::New(key(i)));
        };
        let Ref::Id(cid) = channel else {
            post(b, "");
            continue;
        };
        listed.insert(cid);
        let mine: &[LiveMessage] = by_channel.get(&cid).map_or(&[], Vec::as_slice);
        let by_content = || mine.iter().find(|l| !used.contains(&l.id) && same_content(&want, l));
        let found = match m.id {
            Some(id) => mine.iter().find(|l| l.id == id).or_else(|| if b.foreign() { by_content() } else { None }),
            None => by_content(),
        };
        let Some(l) = found else {
            if let Some(id) = m.id.filter(|_| !b.foreign()) {
                b.warnings.push(format!(
                    "{path}: message {id} isn't one of the bot's messages in {channel} (deleted, or posted by someone \
                     else); remove its id to post it again"
                ));
            } else {
                post(b, "");
            }
            continue;
        };
        used.insert(l.id);
        if m.id != Some(l.id) {
            b.write_id(path.clone(), "id", Written::Id(l.id));
        }
        let mut ch = Vec::new();
        if want.content.as_deref().unwrap_or("") != l.content {
            changed(&mut ch, "content");
        }
        if !same_embeds(&want.embeds, &l.embeds) {
            changed(&mut ch, "embeds");
        }
        if !same_components(want.components.as_ref(), l.components.as_ref()) {
            changed(&mut ch, "components_v2");
        }
        let flags_mask = SUPPRESS_EMBEDS;
        if m.suppress_embeds.is_some() && (want.flags & flags_mask) != (l.flags & flags_mask) {
            changed(&mut ch, "suppress_embeds");
        }
        if !ch.is_empty() {
            b.op(
                Phase::Messages,
                summary(format!("edit {}", label(m, &channel)), &ch),
                Risk::Safe,
                Ref::Id(l.id),
                OpKind::EditMessage { channel: cid, id: l.id, changes: ch, body: want.clone() },
            );
        }
        if m.pinned != l.pinned {
            b.op(
                Phase::Messages,
                format!("{} {}", if m.pinned { "pin" } else { "unpin" }, label(m, &channel)),
                Risk::Safe,
                Ref::Id(l.id),
                OpKind::SetPinned { channel: cid, id: l.id, pinned: m.pinned },
            );
        }
    }
    if !b.spec.prune.messages {
        return;
    }
    for cid in listed {
        for l in by_channel.get(&cid).into_iter().flatten().filter(|l| !used.contains(&l.id)) {
            b.op(
                Phase::DeleteMessages,
                format!("delete the bot's message {} in {cid}", l.id),
                Risk::Destructive,
                Ref::Id(l.id),
                OpKind::DeleteMessage { channel: cid, id: l.id },
            );
        }
    }
}
