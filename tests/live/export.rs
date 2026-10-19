//! Quickstart §1: export round trip (US1, SC-001).

use crate::{live, run, temp_file};

#[test]
fn export_validates_and_plans_to_nothing() {
    let Some(g) = live() else { return };
    let doctor = run(&["doctor", "-g", &g], &[], "");
    assert_eq!(doctor.code, Some(0), "doctor:\n{}", doctor.stdout);

    let first = run(&["export", "-g", &g], &[], "");
    assert_eq!(first.code, Some(0), "{}", first.stderr);
    let file = temp_file("lab.toml", &first.stdout);
    let v = run(&["validate", &file], &[], "");
    assert_eq!(v.code, Some(0), "{}", v.stdout);
    let p = run(&["plan", "-g", &g, &file, "--detailed-exitcode"], &[], "");
    assert_eq!(p.code, Some(0), "an unchanged export plans to nothing:\n{}", p.stdout);
    assert!(p.stdout.starts_with("No changes."), "{}", p.stdout);

    let second = run(&["export", "-g", &g], &[], "");
    assert_eq!(first.stdout, second.stdout, "exporting twice is byte-identical");
}
