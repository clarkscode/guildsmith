//! T090: ids are written back into the spec file (research E9): only `id = "…"` / `code = "…"`
//! lines are added, everything else (comments, spacing, order) is kept byte for byte, and a
//! second apply writes nothing.

use std::path::PathBuf;

use guildsmith_app::App;
use guildsmith_app::assets::Caller;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_app::services::apply::{self, Progress, WrittenId};
use guildsmith_app::services::plan;
use guildsmith_app::writeback;
use guildsmith_core::Id;
use guildsmith_core::spec::SpecPath;

const SPEC: &str = r##"# My server   (comments survive)
[guild]
name = "Lab"   # informational

[[categories]]
name = "Community"

  [[categories.channels]]
  name = "general"
  kind = "text"

  [[categories.channels]]
  name = "welcome"
  kind = "text"

[[webhooks]]
name    = "CI"          # aligned on purpose
channel = "#general"

[[invites]]
channel = "#welcome"
max_age = 0   # never expires

[[messages]]
channel = "#welcome"
content = """
Hello!
"""
"##;

fn id(path: SpecPath, field: &str, value: &str) -> WrittenId {
    WrittenId { path: path.to_string(), field: field.to_owned(), value: value.to_owned(), spec_path: path }
}

/// The text with the lines that `after` added taken out again.
fn without_added(after: &str, before: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    after
        .lines()
        .filter(|l| old.contains(l) || !(l.trim_start().starts_with("id = ") || l.trim_start().starts_with("code = ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn only_id_lines_are_added() {
    let ids = [
        id(SpecPath::root().key("webhooks").index(0), "id", "123"),
        id(SpecPath::root().key("invites").index(0), "code", "abc"),
        id(SpecPath::root().key("messages").index(0), "id", "456"),
    ];
    let (new, n) = writeback::apply_ids(SPEC, &ids).unwrap();
    assert_eq!(n, 3);
    assert!(new.contains("id = \"123\"") && new.contains("code = \"abc\"") && new.contains("id = \"456\""), "{new}");
    assert_eq!(without_added(&new, SPEC), SPEC.trim_end_matches('\n'), "everything else is kept:\n{new}");
    assert_eq!(new.lines().count(), SPEC.lines().count() + 3);
    // Ids lead their table.
    let hook = new.split("[[webhooks]]").nth(1).unwrap();
    assert!(hook.trim_start().starts_with("id = \"123\""), "{hook}");

    let (again, n) = writeback::apply_ids(&new, &ids).unwrap();
    assert_eq!((again.as_str(), n), (new.as_str(), 0), "writing the same ids again changes nothing");
}

#[test]
fn a_different_id_replaces_the_value_only() {
    let text = "[[webhooks]]\nid = \"1\"   # from another server\nname = \"CI\"\nchannel = \"#general\"\n";
    let (new, n) = writeback::apply_ids(text, &[id(SpecPath::root().key("webhooks").index(0), "id", "2")]).unwrap();
    assert_eq!(n, 1);
    assert_eq!(new, text.replace("\"1\"", "\"2\""));
}

#[tokio::test]
async fn apply_then_write_back_then_apply_writes_nothing() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("writeback_apply");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("guild.toml");
    std::fs::write(&file, SPEC).unwrap();
    let app = App::new(FakeDiscord::new(vec![(fixtures::empty(Id(3000), "Lab"), vec![])]), Policy::default());
    let caller = Caller::Cli { spec_dir: Some(&dir) };
    let mut sink = |_: &Progress| {};

    let p = plan::plan_in(&app, Some("3000"), SPEC, caller).await.unwrap();
    let report = apply::apply_in(&app, Some("3000"), SPEC, &p.plan_hash, caller, &mut sink).await.unwrap();
    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(writeback::write_ids(&file, &report.written_ids).unwrap(), 3);
    let written = std::fs::read_to_string(&file).unwrap();
    assert_eq!(without_added(&written, SPEC), SPEC.trim_end_matches('\n'));

    let p = plan::plan_in(&app, Some("3000"), &written, caller).await.unwrap();
    assert!(p.ops.is_empty() && p.ids_to_write.is_empty(), "{:?} {:?}", p.ops, p.ids_to_write);
    assert_eq!(writeback::write_ids(&file, &[]).unwrap(), 0);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), written, "a second apply writes nothing");
}
