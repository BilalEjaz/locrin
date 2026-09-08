mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::dead_file::DeadFile;

#[test]
fn flags_orphans_but_not_cycles() {
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("src/orphan.ts".into(), 1)], "{out:?}");
    assert_eq!(out[0].evidence, "src/orphan.ts is imported nowhere and is not an entry point");
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::Medium));
}

#[test]
fn entry_points_by_package_and_convention_are_clean() {
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn config_entry_points_and_allow_marker_on_line_one() {
    let config = Config { entry_points: vec!["tools/**".into()], ..Config::default() };
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "edge"), &config);
    assert_eq!(hits(&out), vec![("src/gone.ts".into(), 1)], "{out:?}");
}
