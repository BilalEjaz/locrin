use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::prelude::*;

/// The five files `init` is responsible for, in the order it reports them.
const FILES: [&str; 5] =
    ["locrin.toml", ".claude/settings.json", ".mcp.json", ".git/hooks/pre-commit", "locrin-baseline.json"];

const CLEAN_TS: &str = "export function ok(): number {\n  return 1;\n}\n";

fn locrin(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("locrin").unwrap();
    c.current_dir(dir).env("LOCRIN_CACHE_DIR", dir.join(".cache"));
    c
}

/// git with the identity, signing and line-ending settings pinned, so a
/// developer's own configuration cannot change what these tests see.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
}

/// A repository holding one clean source file: the count in the progress line is
/// then a fact the test can assert rather than a number that moves with the
/// fixture.
fn temp_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/ok.ts"), CLEAN_TS).unwrap();
    dir
}

fn init(dir: &Path) -> Output {
    let out = locrin(dir).args(["init", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    out
}

/// The same run, expected to fail. Kept apart from `init` so a test that pins
/// the failure path cannot pass by accident on a run that succeeded.
fn failing_init(dir: &Path) -> Output {
    let out = locrin(dir).args(["init", "--offline"]).output().unwrap();
    assert_ne!(out.status.code(), Some(0), "init succeeded:\n{}", String::from_utf8_lossy(&out.stdout));
    out
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

fn has_line(stdout: &str, line: &str) -> bool {
    stdout.lines().any(|l| l == line)
}

fn has_line_starting(stdout: &str, prefix: &str) -> bool {
    stdout.lines().any(|l| l.starts_with(prefix))
}

#[test]
fn init_writes_every_file_and_says_so() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    for rel in FILES {
        assert!(has_line(&stdout, &format!("wrote {rel}")), "no `wrote {rel}` in:\n{stdout}");
        assert!(dir.path().join(rel).exists(), "{rel} was reported but not written");
    }
    assert!(has_line(&stdout, "baseline written with 0 finding(s)"), "{stdout}");

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Edit|Write|MultiEdit", "{settings}");
    assert_eq!(settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "locrin hook post-edit", "{settings}");
    assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "locrin hook stop", "{settings}");

    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["locrin"]["command"], "locrin", "{mcp}");

    // Progress is what a person watching the first scan reads, and it belongs on
    // stderr: it names no file the command touched.
    let stderr = stderr_of(&out);
    assert!(stderr.contains("indexing 1 source file(s)"), "{stderr}");
    assert!(stderr.contains("indexed 1 file(s) in"), "{stderr}");
}

#[test]
fn init_twice_changes_nothing() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    init(dir.path());
    let before: Vec<Vec<u8>> = FILES.iter().map(|rel| std::fs::read(dir.path().join(rel)).unwrap()).collect();

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().count(), FILES.len(), "{stdout}");
    for line in stdout.lines() {
        assert!(line.starts_with("unchanged "), "{stdout}");
    }
    let after: Vec<Vec<u8>> = FILES.iter().map(|rel| std::fs::read(dir.path().join(rel)).unwrap()).collect();
    assert_eq!(before, after, "a second init rewrote a file");
}

#[test]
fn init_leaves_an_existing_pre_commit_alone() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    let hook = dir.path().join(".git/hooks/pre-commit");
    let mine = "#!/bin/sh\necho mine\n";
    std::fs::write(&hook, mine).unwrap();

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert!(has_line_starting(&stdout, "skipped .git/hooks/pre-commit:"), "{stdout}");
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), mine, "init overwrote a hook it did not write");
    assert!(has_line(&stdout, "wrote locrin.toml"), "{stdout}");
}

#[test]
fn init_without_git_skips_the_hook() {
    let dir = temp_repo();

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert!(
        has_line(&stdout, "skipped .git/hooks/pre-commit: not a git repository: no pre-commit hook installed"),
        "{stdout}"
    );
    for rel in ["locrin.toml", ".claude/settings.json", ".mcp.json", "locrin-baseline.json"] {
        assert!(has_line(&stdout, &format!("wrote {rel}")), "{stdout}");
        assert!(dir.path().join(rel).exists(), "{rel} was reported but not written");
    }
}

/// A repository that already runs its hooks through husky owns `.git/hooks`
/// itself, so init leaves it alone and says where the command belongs instead.
#[test]
fn init_defers_to_husky() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    std::fs::create_dir_all(dir.path().join(".husky")).unwrap();

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert!(
        has_line(
            &stdout,
            "skipped .git/hooks/pre-commit: husky detected: add `locrin hook pre-commit` to .husky/pre-commit"
        ),
        "{stdout}"
    );
    assert!(!dir.path().join(".git/hooks/pre-commit").exists(), "init installed a hook husky owns");
}

#[test]
fn init_does_not_reset_an_existing_baseline() {
    let dir = temp_repo();
    let baseline = dir.path().join("locrin-baseline.json");
    let mine = r#"{"entries":[{"id":"abc123","rule":"leftover-debug","file":"src/ok.ts","reason":"kept","author":"t","date":"2026-09-10"}]}"#;
    std::fs::write(&baseline, mine).unwrap();

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert!(has_line(&stdout, "unchanged locrin-baseline.json"), "{stdout}");
    assert!(!stdout.contains("baseline written with"), "{stdout}");
    let text = std::fs::read_to_string(&baseline).unwrap();
    assert!(text.contains("abc123"), "init replaced a baseline it did not write: {text}");
}

/// The report on stdout is the only record of what init did, so a run that
/// cannot finish must not have created anything: files written before the
/// failure would never be named anywhere.
#[test]
fn init_with_a_broken_config_writes_nothing() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    std::fs::write(dir.path().join("locrin.toml"), "nonsense = 1\n").unwrap();

    let out = failing_init(dir.path());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("locrin.toml"), "the error does not name the file to fix:\n{stderr}");
    for rel in [".claude/settings.json", ".mcp.json", ".git/hooks/pre-commit", "locrin-baseline.json"] {
        assert!(!dir.path().join(rel).exists(), "{rel} was written by a run that failed");
    }
}

/// Same rule for the other file init reads before it decides: the settings file
/// is checked before the config template is written, not after.
#[test]
fn init_with_a_non_object_settings_file_writes_nothing() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    std::fs::write(dir.path().join(".claude/settings.json"), "[]").unwrap();

    let out = failing_init(dir.path());
    let stderr = stderr_of(&out);
    assert!(stderr.contains(".claude/settings.json"), "the error does not name the file to fix:\n{stderr}");
    for rel in ["locrin.toml", ".mcp.json"] {
        assert!(!dir.path().join(rel).exists(), "{rel} was written by a run that failed");
    }
}
