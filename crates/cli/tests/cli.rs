use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
}

fn copy_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in walkdir(&fixture()) {
        let rel = entry.strip_prefix(fixture()).unwrap();
        let dest = dir.path().join(rel);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &dest).unwrap();
    }
    dir
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn locrin(dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("locrin").unwrap();
    c.current_dir(dir).env("LOCRIN_CACHE_DIR", dir.join(".cache"));
    c
}

#[test]
fn check_blocks_on_debug_and_reports_marker() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("BLOCK  2 finding(s)"), "{text}");
    assert!(text.contains("src/dirty.ts"));
    assert!(text.contains("leftover-debug"));
    assert!(text.contains("leftover-agent-marker"));
}

#[test]
fn json_output_is_compact_and_capped() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "block");
    assert_eq!(v["blocking"], 1);
    assert_eq!(v["findings"][0]["rule"], "leftover-debug");
    assert!(v["findings"][0].get("source").is_none());
}

#[test]
fn baseline_create_then_check_passes_and_changed_only_sees_edits() {
    let dir = copy_fixture();
    locrin(dir.path()).args(["baseline", "create"]).assert().success();
    assert!(dir.path().join("locrin-baseline.json").exists());
    locrin(dir.path()).arg("check").assert().code(0);

    // no edits since the last index: --changed finds nothing to check
    let out = locrin(dir.path()).args(["check", "--changed"]).output().unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("PASS  0 finding(s)"));

    // introduce a new debug line in clean.ts; only that file is in scope and it blocks
    std::fs::write(
        dir.path().join("src/clean.ts"),
        "export function ok(): number {\n  console.log(\"new\");\n  return 1;\n}\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--changed"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("src/clean.ts"));
    assert!(!text.contains("src/dirty.ts"));
}

#[test]
fn explicit_path_limits_scope() {
    let dir = copy_fixture();
    locrin(dir.path()).args(["check", "src/clean.ts"]).assert().code(0);
}

#[test]
fn baseline_accept_by_id() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["findings"][0]["id"].as_str().unwrap().to_string();
    locrin(dir.path()).args(["baseline", "accept", &id, "--reason", "legacy"]).assert().success();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["blocking"], 0);
    assert_eq!(v["status"], "advisory");
}

#[test]
fn engine_error_exits_two() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("locrin.toml"), "excludes = [").unwrap();
    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("error:"));
}
