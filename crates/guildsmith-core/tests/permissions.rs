//! T012: permission names ↔ bits, canonical serialization, unknown bits, Discord strings.

use guildsmith_core::{Permission, PermissionSet};
use proptest::prelude::*;

#[test]
fn table_covers_bits_0_to_52_except_47() {
    let bits: Vec<u8> = Permission::ALL.iter().map(|p| p.bit()).collect();
    let expected: Vec<u8> = (0..=52).filter(|b| *b != 47).collect();
    assert_eq!(bits, expected);
    assert!(Permission::from_bit(47).is_none());
    assert_eq!(Permission::from_bit(48), Some(Permission::SetVoiceChannelStatus));
}

#[test]
fn every_name_round_trips() {
    for p in Permission::ALL {
        assert_eq!(Permission::from_name(p.name()), Some(*p), "{p:?}");
        assert_eq!(Permission::from_bit(p.bit()), Some(*p), "{p:?}");
        assert!(p.name().chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{} is snake_case", p.name());
    }
}

#[test]
fn serializes_as_sorted_snake_case_names() {
    let set: PermissionSet =
        [Permission::ViewChannel, Permission::Administrator, Permission::PinMessages].into_iter().collect();
    let json = serde_json::to_string(&set).unwrap();
    assert_eq!(json, r#"["administrator","pin_messages","view_channel"]"#);
}

#[test]
fn unknown_bits_survive_a_round_trip() {
    let raw = (1u64 << 47) | (1 << 60) | (1 << Permission::Speak.bit());
    let set = PermissionSet::from_bits(raw);
    let names = set.names();
    assert_eq!(names, vec!["speak", "unknown_47", "unknown_60"]);
    let back: PermissionSet = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
    assert_eq!(back.bits(), raw);
}

#[test]
fn unknown_names_are_rejected() {
    let err = serde_json::from_str::<PermissionSet>(r#"["view_channels"]"#).unwrap_err();
    assert!(err.to_string().contains("unknown permission `view_channels`"), "{err}");
}

#[test]
fn discord_string_form() {
    let set = PermissionSet::from_discord_str("2147483648").unwrap();
    assert!(set.contains(Permission::UseApplicationCommands));
    assert_eq!(set.to_discord_string(), "2147483648");
    assert!(PermissionSet::from_discord_str("-1").is_err());
    assert!(PermissionSet::from_discord_str("abc").is_err());
}

proptest! {
    #[test]
    fn any_bits_round_trip_through_names(bits in any::<u64>()) {
        let set = PermissionSet::from_bits(bits);
        let json = serde_json::to_string(&set).unwrap();
        let back: PermissionSet = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.bits(), bits);
    }

    #[test]
    fn any_bits_round_trip_through_discord_string(bits in any::<u64>()) {
        let set = PermissionSet::from_bits(bits);
        prop_assert_eq!(PermissionSet::from_discord_str(&set.to_discord_string()).unwrap(), set);
    }

    #[test]
    fn names_are_sorted(bits in any::<u64>()) {
        let names = PermissionSet::from_bits(bits).names();
        let mut sorted = names.clone();
        sorted.sort();
        prop_assert_eq!(names, sorted);
    }
}
