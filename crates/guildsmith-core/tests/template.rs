//! T075: template import snapshots. Three public-template fixtures are imported; each imported
//! spec passes validation (SC-009), placeholder ids become names, and the output has no ids.

use guildsmith_core::spec::writer::{TemplateHeader, write_template};
use guildsmith_core::spec::{GuildSpec, Target};
use guildsmith_core::template;
use guildsmith_core::validate::{Severity, validate_text};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/templates/{name}.json", env!("CARGO_MANIFEST_DIR")))
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn import(name: &str) -> (GuildSpec, String) {
    let t = fixture(name);
    let (spec, notes) = template::import(&t["serialized_source_guild"]).expect("imports");
    let header = TemplateHeader {
        version: "0.0.0-test",
        code: t["code"].as_str().unwrap(),
        template_name: t["name"].as_str().unwrap(),
        date: "2026-10-20",
    };
    let text = write_template(&spec, &header, &notes);
    (spec, text)
}

fn all_targets(spec: &GuildSpec) -> Vec<Target> {
    let mut out = Vec::new();
    for c in &spec.categories {
        out.extend(c.overwrites.iter().flatten().map(|o| o.target.clone()));
        for ch in &c.channels {
            out.extend(ch.overwrites.iter().flatten().map(|o| o.target.clone()));
        }
    }
    for ch in &spec.channels {
        out.extend(ch.overwrites.iter().flatten().map(|o| o.target.clone()));
    }
    out
}

#[test]
fn every_template_imports_to_a_valid_spec_without_ids() {
    for name in ["gaming", "study", "community"] {
        let (spec, text) = import(name);
        let report = validate_text(&text);
        let errors: Vec<_> = report.problems.iter().filter(|p| p.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "{name}: {errors:#?}\n{text}");
        assert!(!text.lines().any(|l| l.trim_start().starts_with("id =")), "{name} has ids:\n{text}");
        assert!(
            all_targets(&spec).iter().all(|t| matches!(t, Target::Everyone | Target::RoleName(_))),
            "{name}: placeholder ids left"
        );
        insta::assert_snapshot!(format!("template_{name}"), text);
    }
}

#[test]
fn placeholders_become_role_names_and_unmanaged_kinds_are_noted() {
    let (spec, text) = import("community");
    let announcements = spec.channels.iter().find(|c| c.name == "announcements").unwrap();
    let targets: Vec<String> = announcements.overwrites.iter().flatten().map(|o| o.target.to_string()).collect();
    assert_eq!(targets, ["@everyone", "@Organizers"]);
    assert!(text.contains("# unmanaged: \"directory\" (channel type 14)"), "{text}");
    assert_eq!(spec.roles.first().map(|r| r.name.as_str()), Some("Admins"), "highest first");
    let (study, _) = import("study");
    let forum = study.categories[1].channels.iter().find(|c| c.name == "questions").unwrap();
    assert_eq!(forum.tags.as_ref().unwrap().iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["math", "answered"]);
    assert_eq!(forum.default_reaction.as_deref(), Some("👍"));
}

#[test]
fn a_broken_template_is_an_error() {
    assert!(template::import(&serde_json::json!({"roles": "nope"})).is_err());
}
