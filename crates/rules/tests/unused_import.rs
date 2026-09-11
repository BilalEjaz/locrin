mod common;

use common::{fixture, hits, run_on, run_on_langs};
use locrin_core::config::{Config, Languages};
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

/// `unused-import` reads the TypeScript grammar's import nodes, so it declares
/// the JavaScript family and the runner never hands it a PHP file. A PHP file
/// whose `use` statements are unused is not this rule's business, even when the
/// repository has asked for PHP.
#[test]
fn php_use_statements_are_not_this_rules_business() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(UnusedImport), &fixture("unused_import", "php/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}
