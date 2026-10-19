//! T013: the fingerprint ignores list order and changes with any field.

mod common;

use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::{Id, Permission};

type Edit = Box<dyn Fn(&mut Snapshot)>;

#[test]
fn list_order_does_not_change_the_fingerprint() {
    let a = common::lab();
    let mut b = common::lab();
    b.roles.reverse();
    b.channels.reverse();
    for c in &mut b.channels {
        c.overwrites.reverse();
    }
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.normalized(), b.normalized());
}

#[test]
fn fingerprint_is_64_hex_chars() {
    let fp = common::lab().fingerprint();
    assert_eq!(fp.len(), 64);
    assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn any_field_change_changes_the_fingerprint() {
    let base = common::lab().fingerprint();
    let edits: Vec<Edit> = vec![
        Box::new(|s| s.guild.name.push('!')),
        Box::new(|s| s.roles[1].name.push('!')),
        Box::new(|s| s.roles[1].color = 0x3B_A5_5D),
        Box::new(|s| s.roles[1].permissions.insert(Permission::BanMembers)),
        Box::new(|s| s.roles[1].position += 1),
        Box::new(|s| s.channels[1].topic = Some("hi".to_owned())),
        Box::new(|s| s.channels[1].parent_id = None),
        Box::new(|s| s.channels[1].position += 1),
        Box::new(|s| {
            s.channels[2].overwrites[0].allow.insert(Permission::SendMessages);
        }),
        Box::new(|s| s.bot.role_ids.push(Id(11))),
    ];
    for (i, edit) in edits.iter().enumerate() {
        let mut s = common::lab();
        edit(&mut s);
        assert_ne!(s.fingerprint(), base, "edit #{i} should change the fingerprint");
    }
}
