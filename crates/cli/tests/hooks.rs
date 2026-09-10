mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

use common::{copy_fixture, locrin};

/// One of the recorded payloads with the two paths that name a real place on
/// this machine rewritten to the temporary copy of the fixture repository.
/// Rewriting through `serde_json` rather than by string substitution is what
/// keeps a Windows path with its backslashes correctly escaped.
fn payload(name: &str, dir: &Path, file_path: &str) -> String {
    let raw =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks").join(name)).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["cwd"] = serde_json::Value::String(dir.display().to_string());
    v["tool_input"]["file_path"] = serde_json::Value::String(file_path.to_string());
    v.to_string()
}

/// Runs the hook the way Claude Code does: the payload on stdin, the project
/// directory as the working directory, nothing on the command line.
fn post_edit(dir: &Path, payload: &str) -> Output {
    let mut child = locrin(dir)
        .args(["hook", "post-edit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
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
