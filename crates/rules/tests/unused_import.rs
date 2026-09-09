mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::unused_import::UnusedImport;

#[test]
fn flags_every_unused_binding_including_type_namespace_and_default() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("a.ts".into(), 1), ("a.ts".into(), 2), ("a.ts".into(), 3), ("a.ts".into(), 4)]);
    assert_eq!(out[0].evidence, "`unused` is imported from \"./lib\" but never used");
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::High));
    assert_eq!(out[0].rule, "unused-import");
}

#[test]
fn jsx_types_shorthand_export_clauses_and_aliases_count_as_use() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn allow_marker_shadowing_jsx_pragma_and_export_from() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "edge"), &Config::default());
    // Files come in walk order: d.ts, e.tsx, f.ts. e.tsx is clean (pragma factory).
    assert_eq!(hits(&out), vec![("d.ts".into(), 3), ("f.ts".into(), 1)], "{out:?}");
}
