mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::{copy_fixture, locrin};

/// One of the recorded payloads, as JSON to be rewritten before it is sent.
fn read_payload(name: &str) -> serde_json::Value {
    let raw =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks").join(name)).unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// One of the recorded payloads with the two paths that name a real place on
/// this machine rewritten to the temporary copy of the fixture repository.
/// Rewriting through `serde_json` rather than by string substitution is what
/// keeps a Windows path with its backslashes correctly escaped.
fn payload(name: &str, dir: &Path, file_path: &str) -> String {
    let mut v = read_payload(name);
    v["cwd"] = serde_json::Value::String(dir.display().to_string());
    v["tool_input"]["file_path"] = serde_json::Value::String(file_path.to_string());
    v.to_string()
}

/// A Stop payload, which names no file: the hook's scope is the whole working
/// tree, so `cwd` is the only path in it that has to be rewritten.
fn stop_payload(name: &str, dir: &Path) -> String {
    let mut v = read_payload(name);
    v["cwd"] = serde_json::Value::String(dir.display().to_string());
    v.to_string()
}

/// Runs a hook the way Claude Code does: the payload on stdin, the project
/// directory as the working directory, nothing on the command line.
fn run_hook(dir: &Path, subcommand: &str, payload: &str) -> Output {
    let mut child = locrin(dir)
        .args(["hook", subcommand])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn post_edit(dir: &Path, payload: &str) -> Output {
    run_hook(dir, "post-edit", payload)
}

fn stop(dir: &Path, payload: &str) -> Output {
    run_hook(dir, "stop", payload)
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn post_edit_blocks_on_a_dirty_file() {
    let dir = copy_fixture();
    let file = dir.path().join("src/dirty.ts");
    let out = post_edit(dir.path(), &payload("post_edit_write.json", dir.path(), &file.display().to_string()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout_of(&out);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(v["decision"], "block", "{text}");
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.starts_with("BLOCK"), "{reason}");
    assert!(reason.contains("leftover-debug"), "{reason}");
    assert!(reason.contains("src/dirty.ts:"), "{reason}");
}

#[test]
fn post_edit_is_silent_on_a_clean_file() {
    let dir = copy_fixture();
    let file = dir.path().join("src/clean.ts");
    let out = post_edit(dir.path(), &payload("post_edit_write.json", dir.path(), &file.display().to_string()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout_of(&out), "");
}

#[test]
fn post_edit_ignores_a_markdown_file() {
    let dir = copy_fixture();
    let file = dir.path().join("README.md");
    // The file exists, so what stops the hook is the extension and not a
    // missing path.
    std::fs::write(&file, "# repo\n\nNotes about the repository.\n").unwrap();
    let out = post_edit(dir.path(), &payload("post_edit_markdown.json", dir.path(), &file.display().to_string()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout_of(&out), "");
}

#[test]
fn post_edit_survives_garbage_stdin() {
    let dir = copy_fixture();
    let out = post_edit(dir.path(), "not json");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout_of(&out), "");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("locrin"), "{err}");
}

/// Claude Code sends native separators, so on Windows the payload carries a
/// path full of backslashes. Nothing translates them; this proves it.
#[test]
#[cfg(windows)]
fn post_edit_accepts_a_windows_path() {
    let dir = copy_fixture();
    let file = format!("{}\\src\\dirty.ts", dir.path().display());
    assert!(!file.contains('/'), "{file}");
    let out = post_edit(dir.path(), &payload("post_edit_edit_windows.json", dir.path(), &file));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout_of(&out);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(v["decision"], "block", "{text}");
}

/// git as these tests drive it. The identity and the signing setting are pinned
/// so a machine configured for a real developer still commits, and line endings
/// are left alone so the bytes committed are the bytes written.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
}

/// The fixture repository with one commit behind it, which is the state the Stop
/// hook is written for: `HEAD` exists, and the working tree starts clean.
/// `src/dirty.ts` blocks a whole-repository check and being committed is exactly
/// what keeps it out of the working-tree view.
fn committed_fixture() -> tempfile::TempDir {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "base"]);
    dir
}

const CLEAN_TS: &str = "export function ok(): number {\n  return 1;\n}\n";
const EDITED_CLEAN_TS: &str = "export function ok(): number {\n  console.log(\"ok\");\n  return 1;\n}\n";

fn json_of(out: &Output) -> serde_json::Value {
    let text = stdout_of(out);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[test]
fn stop_is_silent_when_nothing_changed() {
    let dir = committed_fixture();
    let out = stop(dir.path(), &stop_payload("stop.json", dir.path()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout_of(&out), "");
}

#[test]
fn stop_sends_the_agent_back_up_to_three_times_then_tells_the_human() {
    let dir = committed_fixture();
    std::fs::write(dir.path().join("src/clean.ts"), EDITED_CLEAN_TS).unwrap();

    for round in 1..=3 {
        // The first stop is the agent's own; every stop after a block carries
        // stop_hook_active, which changes nothing here because the round counter
        // is this hook's own loop guard.
        let name = if round == 1 { "stop.json" } else { "stop_active.json" };
        let out = stop(dir.path(), &stop_payload(name, dir.path()));
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let v = json_of(&out);
        assert_eq!(v["decision"], "block", "{v}");
        let reason = v["reason"].as_str().unwrap();
        assert!(reason.contains("leftover-debug"), "{reason}");
        assert!(reason.contains("src/clean.ts:"), "{reason}");
        assert!(reason.contains(&format!("Round {round} of 3")), "{reason}");
    }

    // The fourth stop stands down, and says so to the person rather than to the
    // agent: the findings are still there, but sending it back a fourth time is
    // how a hook turns into a loop.
    let out = stop(dir.path(), &stop_payload("stop_active.json", dir.path()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json_of(&out);
    assert!(v["decision"].is_null(), "{v}");
    assert_eq!(
        v["systemMessage"].as_str().unwrap(),
        "locrin: 1 blocking finding(s) remain after 3 rounds; the agent was not sent back again. \
         Run `locrin check --base HEAD` to see them."
    );

    // And from the fifth on it has nothing left to say: the person was told
    // once.
    let out = stop(dir.path(), &stop_payload("stop_active.json", dir.path()));
    assert_eq!(stdout_of(&out), "");
}

#[test]
fn a_clean_verdict_resets_the_rounds() {
    let dir = committed_fixture();
    let file = dir.path().join("src/clean.ts");
    std::fs::write(&file, EDITED_CLEAN_TS).unwrap();
    let first = stdout_of(&stop(dir.path(), &stop_payload("stop.json", dir.path())));
    assert!(first.contains("Round 1 of 3"), "{first}");

    // The agent fixed it, so the loop is closed and the count goes with it.
    std::fs::write(&file, CLEAN_TS).unwrap();
    assert_eq!(stdout_of(&stop(dir.path(), &stop_payload("stop_active.json", dir.path()))), "");

    // A later edit in the same session therefore starts again at one rather than
    // inheriting a spent budget.
    std::fs::write(&file, EDITED_CLEAN_TS).unwrap();
    let again = stdout_of(&stop(dir.path(), &stop_payload("stop.json", dir.path())));
    assert!(again.contains("Round 1 of 3"), "{again}");
}

#[test]
fn stop_uses_changed_scope_without_a_head() {
    // No repository at all, so there is no HEAD to diff against and the hook
    // falls back to what the index calls changed.
    let dir = copy_fixture();
    let scan = locrin(dir.path()).args(["scan", "--offline"]).output().unwrap();
    assert!(scan.status.success(), "{}", String::from_utf8_lossy(&scan.stderr));

    std::fs::write(dir.path().join("src/clean.ts"), EDITED_CLEAN_TS).unwrap();
    let out = stop(dir.path(), &stop_payload("stop.json", dir.path()));
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json_of(&out);
    assert_eq!(v["decision"], "block", "{v}");
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.contains("src/clean.ts:"), "{reason}");
    // src/dirty.ts is untouched since the scan, so the run that answers only for
    // what changed leaves it alone even though a whole-repository check reports
    // it.
    assert!(!reason.contains("src/dirty.ts"), "{reason}");
}

/// The pre-commit hook as git runs it: no payload on stdin, the repository as
/// the working directory, and the exit code is the whole answer.
fn pre_commit(dir: &Path) -> Output {
    locrin(dir).args(["hook", "pre-commit"]).output().unwrap()
}

/// The fixture repository as a git repository with `paths` staged and nothing
/// committed, which is the state a pre-commit hook runs in: the index is what is
/// about to become a commit, and the rest of the working tree is not its
/// business.
fn staged_fixture(paths: &[&str]) -> tempfile::TempDir {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    for p in paths {
        git(dir.path(), &["add", p]);
    }
    dir
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn pre_commit_blocks_when_a_staged_file_blocks() {
    let dir = staged_fixture(&["src/dirty.ts"]);
    let out = pre_commit(dir.path());
    // 1 and not 0: unlike the agent hooks, this one's exit code is the verdict,
    // and a non-zero one is how git is told to stop the commit.
    assert_eq!(out.status.code(), Some(1), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.starts_with("BLOCK"), "{text}");
    assert!(text.contains("leftover-debug"), "{text}");
    assert!(text.contains("src/dirty.ts"), "{text}");
}

#[test]
fn pre_commit_passes_a_clean_stage() {
    let dir = staged_fixture(&["src/clean.ts"]);
    let out = pre_commit(dir.path());
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.starts_with("PASS  0 finding(s)"), "{text}");
}

/// A file the engine does not parse is staged, so the run has nothing to report
/// for it. It still runs: the lockfile is not source either, and it is exactly
/// the file a commit most wants checked.
#[test]
fn pre_commit_ignores_a_staged_file_that_is_not_source() {
    let dir = staged_fixture(&["package.json"]);
    let out = pre_commit(dir.path());
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.starts_with("PASS  0 finding(s)"), "{text}");
    assert!(!text.contains("src/dirty.ts"), "{text}");
}

/// `git commit` runs the hook before it notices there is nothing to commit, and
/// so does `git commit --amend`. Saying so is better than printing a verdict
/// about nothing.
#[test]
fn pre_commit_says_when_nothing_is_staged() {
    let dir = staged_fixture(&[]);
    let out = pre_commit(dir.path());
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "locrin: nothing staged to check\n");
}

/// The working tree is full of findings the commit does not carry, and the gate
/// answers for the commit. Both `src/dirty.ts` and the file written below block
/// a whole-repository check, and neither is staged.
#[test]
fn pre_commit_ignores_unstaged_dirt() {
    let dir = staged_fixture(&["src/clean.ts"]);
    std::fs::write(dir.path().join("src/extra.ts"), "export const n = 1;\nconsole.log(n);\n").unwrap();
    let out = pre_commit(dir.path());
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.starts_with("PASS  0 finding(s)"), "{text}");
    assert!(!text.contains("src/extra.ts"), "{text}");
    assert!(!text.contains("src/dirty.ts"), "{text}");
}

/// A file staged and then removed from the working tree names a path that is not
/// there, and a named path that does not exist is an error the run refuses to
/// start on. The commit is a legitimate one, so the hook drops the path rather
/// than failing over it.
#[test]
fn pre_commit_skips_a_staged_path_whose_file_is_gone() {
    let dir = staged_fixture(&["src/clean.ts"]);
    std::fs::remove_file(dir.path().join("src/clean.ts")).unwrap();
    let out = pre_commit(dir.path());
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "locrin: nothing staged to check\n");
}
