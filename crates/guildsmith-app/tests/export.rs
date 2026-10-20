//! T041: export snapshot tests (insta) against `FakeDiscord` loaded with the rich fixture
//! (equivalent to the adapter's recorded T014 fixtures).

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::export;
use guildsmith_core::spec;
use guildsmith_core::validate::validate_text;
use guildsmith_core::{Permission, PermissionSet};

const TODAY: &str = "2026-10-19";

fn app(snapshot: guildsmith_core::snapshot::Snapshot) -> App<FakeDiscord> {
    App::new(FakeDiscord::new(vec![(snapshot, fixtures::lab_members())]), Policy::default())
}

#[tokio::test]
async fn export_matches_snapshot() {
    let out = export::export(&app(fixtures::rich()), Some("1000"), TODAY).await.unwrap();
    // Keep the snapshot stable across releases; the header test checks the real version.
    let toml = out.toml.replacen(
        &format!("# Exported by guildsmith {} from ", env!("CARGO_PKG_VERSION")),
        "# Exported by guildsmith <VERSION> from ",
        1,
    );
    insta::assert_snapshot!("rich_export", toml);
}

#[tokio::test]
async fn header_order_comments_and_round_trip() {
    let out = export::export(&app(fixtures::rich()), Some("1000"), TODAY).await.unwrap();
    let t = &out.toml;
    assert!(t.starts_with(&format!(
        "#:schema https://raw.githubusercontent.com/clarkscode/guildsmith/main/schema/guild.schema.json\n\
         # Exported by guildsmith {} from \"Guildsmith Lab\" (1000) on {TODAY}.\n",
        env!("CARGO_PKG_VERSION")
    )));
    // Roles highest first, @everyone last.
    let pos = |needle: &str| t.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    assert!(pos("name = \"Moderator\"") < pos("name = \"Member\""));
    assert!(pos("name = \"Member\"") < pos("name = \"@everyone\""));
    // Managed roles and gradient colors are comments only.
    assert!(!t.contains("name = \"Guildsmith\""));
    assert!(t.contains("# managed (read-only): \"Guildsmith\""));
    assert!(t.contains("# gradient colors (read-only): secondary #FF0000"));
    assert!(!t.contains("secondary_color"));
    // Unsupported kinds are reported, not exported.
    assert!(t.contains("# unmanaged: \"directory\" (channel type 14)"));
    assert_eq!(out.unmanaged, vec!["\"directory\" (channel type 14), id 108".to_owned()]);
    // Parses back to the same spec and validates.
    assert_eq!(spec::parse(t).unwrap(), out.spec);
    let report = validate_text(t);
    assert!(!report.has_errors(), "{:?}", report.problems);
}

#[tokio::test]
async fn exporting_twice_is_byte_identical() {
    let a = app(fixtures::rich());
    let first = export::export(&a, Some("1000"), TODAY).await.unwrap().toml;
    let second = export::export(&a, Some("Guildsmith Lab"), TODAY).await.unwrap().toml;
    assert_eq!(first, second);
}

#[tokio::test]
async fn non_admin_export_warns_hidden_channels_may_be_missing() {
    let mut s = fixtures::rich();
    for r in &mut s.roles {
        if r.id == fixtures::BOT_ROLE {
            r.permissions =
                [Permission::ManageRoles, Permission::ManageChannels].into_iter().collect::<PermissionSet>();
        }
    }
    let out = export::export(&app(s), Some("1000"), TODAY).await.unwrap();
    assert!(out.toml.contains("# WARNING: the bot is not an administrator"), "{}", out.toml);
    assert!(out.warnings.iter().any(|w| w.contains("hidden channels may be missing")));
}

#[tokio::test]
async fn synced_children_export_as_sync_permissions() {
    let mut s = fixtures::rich();
    let cat_overwrites = s.channels.iter().find(|c| c.id == fixtures::MOD_LOG).unwrap().overwrites.clone();
    for c in &mut s.channels {
        if c.id == fixtures::COMMUNITY || c.id == fixtures::GENERAL {
            c.overwrites.clone_from(&cat_overwrites);
        }
    }
    let out = export::export(&app(s), Some("1000"), TODAY).await.unwrap();
    let general = out.spec.categories[0].channels.iter().find(|c| c.name == "general").unwrap();
    assert!(general.sync_permissions);
    assert!(general.overwrites.is_none());
}
