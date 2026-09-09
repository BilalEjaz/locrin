mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::{Boundary, Config};
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::boundary::BoundaryViolation;

fn config() -> Config {
    Config {
        boundaries: vec![
            Boundary {
                name: Some("ui stays off the database".into()),
                from: "src/ui/**".into(),
                forbid: vec!["src/db/**".into()],
                allow: vec![],
            },
            Boundary { name: None, from: "src/db/**".into(), forbid: vec![], allow: vec!["src/shared/**".into()] },
        ],
        ..Config::default()
    }
}

#[test]
fn forbid_and_allow_boundaries_flag_one_finding_per_import_line() {
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &config());
    assert_eq!(hits(&out), vec![("src/db/client.ts".into(), 2), ("src/ui/screen.ts".into(), 1)], "{out:?}");
    let ui = &out[1];
    assert_eq!(
        ui.evidence,
        "src/ui/screen.ts imports \"../db/client\" (src/db/client.ts), which the boundary `ui stays off the database` forbids"
    );
    assert_eq!(ui.related, vec!["src/db/client.ts".to_string()]);
    assert!(out[0].evidence.contains("src/db/** -> outside allow list"), "{}", out[0].evidence);
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
}

#[test]
fn two_boundaries_catching_the_same_line_each_report_it() {
    // Both boundaries start at `src/ui/**`, so both catch `src/ui/screen.ts:1`
    // (the import of `../db/client`): the first because `src/db/**` is forbidden,
    // the second because `src/db/client.ts` is outside its allow list. One
    // finding per import line per boundary means two findings on that line.
    let config = Config {
        boundaries: vec![
            Boundary {
                name: Some("ui stays off the database".into()),
                from: "src/ui/**".into(),
                forbid: vec!["src/db/**".into()],
                allow: vec![],
            },
            Boundary { name: None, from: "src/ui/**".into(), forbid: vec![], allow: vec!["src/shared/**".into()] },
        ],
        ..Config::default()
    };
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &config);
    assert_eq!(hits(&out), vec![("src/ui/screen.ts".into(), 1), ("src/ui/screen.ts".into(), 1)], "{out:?}");
    assert_ne!(out[0].id, out[1].id, "{out:?}");
    assert!(out[0].evidence.contains("`ui stays off the database`"), "{}", out[0].evidence);
    assert!(out[1].evidence.contains("`src/ui/** -> outside allow list`"), "{}", out[1].evidence);
}

#[test]
fn no_boundaries_means_no_findings() {
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &Config::default());
    assert!(out.is_empty());
}

#[test]
fn a_from_glob_may_import_itself_under_allow() {
    let config = Config {
        boundaries: vec![Boundary { name: None, from: "src/**".into(), forbid: vec![], allow: vec![] }],
        ..Config::default()
    };
    // Every import in the fixture stays inside src/**, so an allow list that is
    // empty apart from the implicit "self" rule flags nothing.
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &config);
    assert!(out.is_empty(), "{out:?}");
}
