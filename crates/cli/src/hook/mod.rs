pub mod text;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::sync_channel;
use std::time::Duration;

use locrin_core::lang::Language;
use locrin_core::parse::rel_path;
use locrin_core::walk::{canonical_path, canonical_root};
use serde::Deserialize;
use serde_json::json;

use crate::run;

/// What Claude Code writes to a hook's stdin. Every field this crate reads is
/// optional at the type level so a payload from a newer or older Claude Code
/// still parses; a missing field the hook needs makes the hook a no-op, never
/// an error.
///
/// The whole event is modelled even though `post_edit` reads one field of it:
/// the type is the record of what the agent sends, and the rest of it is what
/// the session hooks read, so the fields are declared here once rather than
/// grown a field at a time as each hook arrives.
#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub session_id: String,
    pub cwd: String,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: ToolInput,
    pub stop_hook_active: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ToolInput {
    pub file_path: String,
}

/// Reads and parses stdin. Unparseable input is a no-op with a warning: the
/// hook cannot know what it was asked about, and stalling the agent over it
/// would be worse than checking nothing.
pub fn read_input() -> Option<Input> {
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        eprintln!("locrin: could not read the hook payload: {e}");
        return None;
    }
    match serde_json::from_str(&buf) {
        Ok(input) => Some(input),
        Err(e) => {
            eprintln!("locrin: could not parse the hook payload: {e}");
            None
        }
    }
}

/// How long a hook may take before it gives up and lets the edit through.
pub const HOOK_BUDGET_MS: u64 = 2000;

/// Runs `f` on a worker thread and waits at most `HOOK_BUDGET_MS`. On timeout
/// returns None; the process exits right after, which is what abandons the
/// worker.
///
/// A worker that panics disconnects the channel, which reads here as the same
/// None: either way the hook has no verdict to report and says so rather than
/// holding the agent.
pub fn watchdog<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Option<anyhow::Result<T>> {
    let (tx, rx) = sync_channel(1);
    std::thread::spawn(move || {
        // A send that fails means the receiver already timed out and left. The
        // result has nowhere to go, and dropping it is exactly what the budget
        // is for.
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_millis(HOOK_BUDGET_MS)).ok()
}

/// Prints one JSON object on stdout (nothing when `v` is None) and returns the
/// exit code, which is always 0.
///
/// Always 0 because a hook's exit code is a channel of its own to Claude Code:
/// a non-zero one is a hook failure, and this hook reports what it found
/// through the object on stdout instead.
pub fn emit(v: Option<serde_json::Value>) -> i32 {
    if let Some(v) = v {
        println!("{v}");
    }
    0
}

/// The file the payload names, once it is a file this engine can say something
/// about: a source file it parses, that exists, and that lives inside the
/// repository the hook was pointed at.
///
/// The path arrives absolute and in the native form of the platform Claude Code
/// is running on, so `PathBuf::from` already reads it correctly and nothing
/// here translates separators.
fn target(root: &Path, file_path: &str) -> Option<PathBuf> {
    if file_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(file_path);
    Language::from_path(&path)?;
    // Canonicalising is both the existence check and the way the containment
    // check below compares like with like: a symlink or a `..` inside the path
    // would otherwise let an edit outside the repository look like one inside
    // it.
    let path = canonical_path(&path).ok()?;
    if !path.starts_with(root) {
        return None;
    }
    Some(path)
}

/// Checks the single file Claude Code just wrote and answers the agent.
///
/// The three shapes of answer are Claude Code's own: `decision: block` makes the
/// agent address the reason before it moves on, `additionalContext` hands it
/// findings without calling the edit an error, and `systemMessage` is for the
/// person watching, used only when the hook could not do its job.
pub fn post_edit(root: &Path, input: Input) -> i32 {
    let root = canonical_root(root);
    let Some(path) = target(&root, &input.tool_input.file_path) else {
        return emit(None);
    };
    let rel = rel_path(&root, &path);
    let opts = run::Options {
        root: root.clone(),
        paths: vec![path],
        changed_only: false,
        json: true,
        // A hook runs on every edit, so it never waits on the network: an
        // advisory fetch that stalls would spend the whole budget.
        offline: true,
        diff: None,
    };
    match watchdog(move || run::check(&opts)) {
        None => emit(Some(json!({
            "systemMessage": format!(
                "locrin: check of {rel} did not finish in {} s; passed without checking it",
                HOOK_BUDGET_MS / 1000
            )
        }))),
        Some(Err(e)) => emit(Some(json!({ "systemMessage": format!("locrin: {e:#}; passed without checking {rel}") }))),
        Some(Ok(v)) if v.blocking > 0 => {
            let v = v.capped(locrin_reporters::agent::CAP);
            emit(Some(json!({ "decision": "block", "reason": text::feedback(&v) })))
        }
        Some(Ok(v)) if !v.findings.is_empty() => {
            let v = v.capped(locrin_reporters::agent::CAP);
            emit(Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": text::feedback(&v),
                }
            })))
        }
        // A clean file says nothing. The hook runs after every edit, and an
        // agent that is told "fine" a hundred times learns to skim what the
        // hook says.
        Some(Ok(_)) => emit(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_payload_and_ignores_fields_it_does_not_model() {
        let input: Input = serde_json::from_str(
            r#"{"session_id":"s1","transcript_path":"/t.jsonl","cwd":"/repo",
                "permission_mode":"default","hook_event_name":"PostToolUse","tool_name":"Write",
                "tool_input":{"file_path":"/repo/src/a.ts","content":"x"},
                "tool_response":{"success":true},"tool_use_id":"toolu_1"}"#,
        )
        .unwrap();
        assert_eq!(input.tool_name, "Write");
        assert_eq!(input.tool_input.file_path, "/repo/src/a.ts");
        assert!(!input.stop_hook_active);
    }

    #[test]
    fn an_empty_payload_still_parses_into_a_no_op() {
        let input: Input = serde_json::from_str("{}").unwrap();
        assert!(input.tool_input.file_path.is_empty());
        assert_eq!(target(Path::new("/repo"), &input.tool_input.file_path), None);
    }

    #[test]
    fn target_skips_a_file_the_engine_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical_root(dir.path());
        let md = root.join("README.md");
        std::fs::write(&md, "# x\n").unwrap();
        assert_eq!(target(&root, &md.display().to_string()), None);
        let ts = root.join("a.ts");
        std::fs::write(&ts, "export const a = 1;\n").unwrap();
        assert_eq!(target(&root, &ts.display().to_string()), Some(canonical_path(&ts).unwrap()));
    }

    #[test]
    fn target_skips_a_file_outside_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let stray = outside.path().join("a.ts");
        std::fs::write(&stray, "export const a = 1;\n").unwrap();
        assert_eq!(target(&canonical_root(dir.path()), &stray.display().to_string()), None);
    }

    #[test]
    fn target_skips_a_path_that_is_not_there() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical_root(dir.path());
        assert_eq!(target(&root, &root.join("gone.ts").display().to_string()), None);
    }

    #[test]
    fn the_watchdog_gives_up_on_a_worker_that_outlasts_the_budget() {
        let slow = watchdog(|| {
            std::thread::sleep(Duration::from_millis(HOOK_BUDGET_MS + 500));
            Ok(1)
        });
        assert!(slow.is_none());
        let quick = watchdog(|| Ok(1)).unwrap().unwrap();
        assert_eq!(quick, 1);
    }
}
