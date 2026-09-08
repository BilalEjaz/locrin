mod common;

use common::{fixture, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::dead_export::DeadExport;

fn view(out: &[locrin_core::finding::Finding]) -> Vec<(String, u32, String)> {
    out.iter().map(|f| (f.file.clone(), f.span.start_line, f.evidence.clone())).collect()
}

#[test]
fn flags_unimported_exports_defaults_and_barrel_reexports() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "flag"), &Config::default());
    assert_eq!(
        view(&out),
        vec![
            ("src/barrel.ts".into(), 1, "`used` is exported from src/barrel.ts but imported nowhere".into()),
            ("src/lib.ts".into(), 4, "`unused` is exported from src/lib.ts but imported nowhere".into()),
            ("src/lib.ts".into(), 7, "`default` is exported from src/lib.ts but imported nowhere".into()),
        ],
        "{out:?}"
    );
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::Medium));
    assert_eq!(out[0].fix, "Remove the export; nothing imports it");
    assert!(out[1].fix.starts_with("Drop the `export` keyword if `unused`"));
}

#[test]
fn entries_aliases_output_extensions_and_default_imports_are_live() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn side_effect_import_keeps_the_file_but_not_its_exports_and_orphans_are_left_to_dead_file() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "edge"), &Config::default());
    assert_eq!(
        view(&out).iter().map(|(f, l, _)| (f.as_str(), *l)).collect::<Vec<_>>(),
        vec![("src/side.ts", 1)],
        "{out:?}"
    );
}
