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

/// The same git, when the test needs what it printed rather than only that it
/// worked.
fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Where git will actually look for this checkout's hooks, as an absolute path:
/// the answer `init` has to arrive at too, whatever `.git` happens to be here.
fn hooks_dir(dir: &Path) -> std::path::PathBuf {
    dir.join(git_stdout(dir, &["rev-parse", "--git-path", "hooks"]))
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

/// In a linked worktree `.git` is a file, not a directory, and the hooks live in
/// the main checkout's admin directory. A hard-coded `.git/hooks` reported "not
/// a git repository" to somebody standing in a perfectly good one.
#[test]
fn init_installs_the_hook_in_a_worktree() {
    let main = temp_repo();
    git(main.path(), &["init", "-q"]);
    git(main.path(), &["add", "."]);
    git(main.path(), &["commit", "-qm", "base"]);
    // Outside the main checkout, so the worktree is not also an untracked
    // directory inside the repository it belongs to.
    let holder = tempfile::tempdir().unwrap();
    let tree = holder.path().join("wt");
    git(main.path(), &["worktree", "add", "-q", &tree.display().to_string(), "-b", "wt"]);
    assert!(tree.join(".git").is_file(), "a linked worktree's .git is a file");

    let out = init(&tree);
    let stdout = stdout_of(&out);
    let hook = hooks_dir(&tree).join("pre-commit");
    assert!(hook.is_file(), "no hook at {}:\n{stdout}", hook.display());
    assert!(has_line_starting(&stdout, "wrote "), "{stdout}");
    let reported = stdout
        .lines()
        .find(|l| l.ends_with("pre-commit"))
        .unwrap_or_else(|| panic!("no pre-commit line in:\n{stdout}"));
    assert!(reported.starts_with("wrote "), "{stdout}");
}

/// `core.hooksPath` moves the hooks somewhere else entirely, and a hook written
/// to `.git/hooks` under it is a file git never runs: init reported `wrote` for
/// a gate that was not installed.
#[test]
fn init_honours_core_hookspath() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "core.hooksPath", ".githooks"]);

    let out = init(dir.path());
    let stdout = stdout_of(&out);
    assert!(dir.path().join(".githooks/pre-commit").is_file(), "{stdout}");
    assert!(!dir.path().join(".git/hooks/pre-commit").exists(), "init wrote a hook git will never run");
    assert!(has_line(&stdout, "wrote .githooks/pre-commit"), "{stdout}");
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

/// The baseline is written from the scan init has just run, not from a second
/// pass over the same tree. The second pass parsed all 1846 files of a real
/// repository again from cold, which doubled `init` and printed every parse
/// warning twice, and a warning repeated is a person hunting for the second file
/// that does not exist.
///
/// The fixture is a genuine parse error rather than a tolerated one: a bare
/// ampersand in JSX text is recovered from, an attribute with no value is not.
#[test]
fn init_parses_the_repository_once() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    std::fs::write(dir.path().join("src/broken.tsx"), "export const a = <Label a=>x</Label>;\n").unwrap();

    let out = init(dir.path());
    let stderr = stderr_of(&out);
    assert_eq!(
        stderr.matches("warning: parse errors in").count(),
        1,
        "the repository was parsed more than once:\n{stderr}"
    );
    // And the progress a person reads still describes one scan, not two.
    assert_eq!(stderr.matches("indexing ").count(), 1, "{stderr}");
    assert_eq!(stderr.matches("indexed ").count(), 1, "{stderr}");
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

/// The rule above only holds while nothing has been written. Once the first file
/// is on disk the report is owed whatever happens next: a scan that cannot run
/// leaves `locrin.toml`, `.claude/settings.json` and `.mcp.json` behind, and
/// spec 5.1's "prints every file it touched" matters most on exactly that path,
/// because nothing else in the repository says those files are locrin's.
///
/// The failure is made after the writes by putting a regular file where the
/// index's cache directory has to go, so opening the index fails on creating it.
#[test]
fn init_reports_what_it_wrote_before_a_late_failure() {
    let dir = temp_repo();
    git(dir.path(), &["init", "-q"]);
    // The helper points LOCRIN_CACHE_DIR here, so a file at that path is a
    // directory the index can never create.
    std::fs::write(dir.path().join(".cache"), "not a directory\n").unwrap();

    let out = failing_init(dir.path());
    let stdout = stdout_of(&out);
    for rel in ["locrin.toml", ".claude/settings.json", ".mcp.json"] {
        assert!(has_line(&stdout, &format!("wrote {rel}")), "no `wrote {rel}` in:\n{stdout}");
        assert!(dir.path().join(rel).exists(), "{rel} was reported but not written");
    }
    // And the failure is still a failure: the person is told why, not just what.
    let stderr = stderr_of(&out);
    assert!(stderr.contains("error:"), "the failure was not reported:\n{stderr}");
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
