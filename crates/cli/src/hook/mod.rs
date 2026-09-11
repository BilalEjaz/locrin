mod session;
pub mod text;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::time::Duration;

use locrin_core::lang::Language;
use locrin_core::parse::rel_path;
use locrin_core::walk::{canonical_path, canonical_root};
use serde::Deserialize;
use serde_json::json;

use crate::run;
use session::{Session, MAX_ROUNDS};

/// What Claude Code writes to a hook's stdin. Every field this crate reads is
/// optional at the type level so a payload from a newer or older Claude Code
/// still parses; a missing field the hook needs makes the hook a no-op, never
/// an error.
///
/// The whole event is modelled even though the hooks act on a few fields of it:
/// the type is the record of what the agent sends, so the fields are declared
/// here once rather than grown one at a time as each hook arrives. The ones no
/// hook acts on say below why they are there anyway.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub session_id: String,
    /// The project directory, which is also where Claude Code runs the hook, so
    /// the process already knows it as its own working directory.
    #[allow(dead_code)]
    pub cwd: String,
    /// Which event this is, which the subcommand already said.
    #[allow(dead_code)]
    pub hook_event_name: String,
    /// Which tool wrote the file. `post_edit` treats Write and Edit alike, so
    /// this is context for a reader of a recorded payload and nothing else.
    #[allow(dead_code)]
    pub tool_name: String,
    pub tool_input: ToolInput,
    /// Whether this stop is already a continuation of one the hook blocked.
    /// Read for the record and deliberately not acted on: `stop` counts its own
    /// rounds, and that counter is the same guard whether the stop is the
    /// agent's own or a continuation of one.
    #[allow(dead_code)]
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

/// How long a hook may take before it gives up and lets the edit through,
/// unless [`HOOK_BUDGET_ENV`] says otherwise.
pub const HOOK_BUDGET_MS: u64 = 2000;

/// The environment variable that overrides the budget for one run.
pub const HOOK_BUDGET_ENV: &str = "LOCRIN_HOOK_BUDGET_MS";

/// The budget this run works to.
///
/// The budget is a person's patience with their editor rather than a property
/// of the engine, and that number is not the same on a laptop and on a CI box
/// three times slower, so it is read from the environment on every hook.
pub fn hook_budget_ms() -> u64 {
    budget_from(std::env::var(HOOK_BUDGET_ENV).ok().as_deref())
}

/// The budget a variable's value asks for, or the default.
///
/// Only a positive number is an override. Unset is someone who never asked for
/// one, empty and unparsable are a typo in a settings file, and zero is a budget
/// no check can finish inside, which would turn every hook into a timeout and
/// leave the repository unchecked without anyone meaning it. None of those is
/// worth failing a hook over either, so each one quietly means the default.
fn budget_from(value: Option<&str>) -> u64 {
    value.and_then(|v| v.trim().parse::<u64>().ok()).filter(|&ms| ms > 0).unwrap_or(HOOK_BUDGET_MS)
}

/// The budget as a message says it to a person: whole seconds for the ordinary
/// two-second one, milliseconds below that, because a sub-second budget rounded
/// to seconds reads as no time at all.
fn budget_text(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{} s", ms / 1000)
    }
}

/// What became of a watchdog worker.
///
/// The two ways of having no verdict are kept apart because they are different
/// news: a timeout is a slow machine or a big file and the budget doing its
/// job, a crash is a bug in this engine. Folding them together would let an
/// engine panic read as a load complaint, and the panic itself is silent
/// (`main` installs an empty panic hook), so this is the only place a crash is
/// visible at all.
pub enum Outcome<T> {
    /// The worker finished inside the budget, with whatever it returned.
    Done(anyhow::Result<T>),
    /// The budget ran out with the worker still going.
    Timeout,
    /// The worker panicked, so there is no result and never will be.
    Crashed,
}

/// Runs `f` on a worker thread and waits at most [`hook_budget_ms`]. A worker
/// that outlives the budget is abandoned by the process exit that follows.
pub fn watchdog<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Outcome<T> {
    let (tx, rx) = sync_channel(1);
    std::thread::spawn(move || {
        // A send that fails means the receiver already timed out and left. The
        // result has nowhere to go, and dropping it is exactly what the budget
        // is for.
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_millis(hook_budget_ms())) {
        Ok(r) => Outcome::Done(r),
        Err(RecvTimeoutError::Timeout) => Outcome::Timeout,
        // The sender is dropped without ever sending only when the worker
        // unwound past it, so a disconnect here is a panic and nothing else.
        Err(RecvTimeoutError::Disconnected) => Outcome::Crashed,
    }
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

/// Runs one of the two agent hooks and turns a panic inside it into a pass.
///
/// The watchdog above catches a panic on the worker thread. A panic anywhere
/// else in the hook, before the worker is spawned or while its verdict is being
/// turned into an answer, is on the process's own thread: it unwinds past the
/// watchdog into `main`'s `catch_unwind` and exits 2. Claude Code reads a Stop
/// hook's exit 2 as "block, and show stderr to the agent", which is precisely
/// the failure spec 9 exists to prevent, and it would repeat on every stop until
/// someone edited the settings file. So the two agent hooks answer their own
/// panics the way they answer every other failure: tell the person, let the work
/// through, exit 0.
///
/// `pre_commit` is deliberately not wrapped. Its exit code is the verdict's, and
/// a broken engine there must stop the commit rather than wave it through.
pub fn guarded(f: impl FnOnce() -> i32) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => emit(Some(json!({
            "systemMessage": "locrin: hook failed inside the engine; passed without checking"
        }))),
    }
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
        Outcome::Timeout => emit(Some(json!({
            "systemMessage": format!(
                "locrin: check of {rel} did not finish in {}; passed without checking it",
                budget_text(hook_budget_ms())
            )
        }))),
        // Named as the engine's own fault, not the machine's: the person
        // reading this should file it, not blame their laptop.
        Outcome::Crashed => emit(Some(json!({
            "systemMessage": format!(
                "locrin: check of {rel} failed inside the engine; passed without checking it"
            )
        }))),
        Outcome::Done(Err(e)) => {
            emit(Some(json!({ "systemMessage": format!("locrin: {e:#}; passed without checking {rel}") })))
        }
        Outcome::Done(Ok(v)) if v.blocking > 0 => {
            let v = v.capped(locrin_reporters::agent::CAP);
            emit(Some(json!({ "decision": "block", "reason": text::feedback(&v) })))
        }
        Outcome::Done(Ok(v)) if !v.findings.is_empty() => {
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
        Outcome::Done(Ok(_)) => emit(None),
    }
}

/// Checks the working tree before Claude Code stops and sends the agent back
/// while blocking findings remain.
///
/// Where `post_edit` answers for the one file an edit touched, this answers for
/// everything the session changed: an agent can leave a repository broken
/// without the last edit being the broken one. The scope is the working tree
/// against HEAD, which is what a person would see in `git status` plus the files
/// they have not added yet, and a repository without a commit falls back to what
/// the index calls changed, because there is nothing to diff against.
///
/// The round counter is what keeps this from being a loop. Three rounds is spec
/// 5.2's cap; on the fourth stop the hook stands down and tells the person
/// instead, because an agent that cannot fix a finding in three tries will not
/// fix it in a fourth, and the person is the one who can decide what to do about
/// it.
pub fn stop(root: &Path, input: Input) -> i32 {
    let root = canonical_root(root);
    let mut session = Session::load(&root, &input.session_id);
    // Past the cap there is nothing left to say: the agent is not being sent
    // back, and the person was told once, on the stop that hit the cap.
    if session.rounds > MAX_ROUNDS {
        return emit(None);
    }
    let diff = crate::git::has_head(&root).then(|| crate::git::DiffScope::Base("HEAD".to_string()));
    let opts = run::Options {
        root: root.clone(),
        paths: vec![],
        changed_only: diff.is_none(),
        json: true,
        offline: true,
        diff,
    };
    match watchdog(move || run::check(&opts)) {
        Outcome::Timeout => emit(Some(json!({
            "systemMessage": format!(
                "locrin: working-tree check did not finish in {}; not blocking the stop",
                budget_text(hook_budget_ms())
            )
        }))),
        Outcome::Crashed => emit(Some(json!({
            "systemMessage": "locrin: working-tree check failed inside the engine; not blocking the stop"
        }))),
        Outcome::Done(Err(e)) => {
            emit(Some(json!({ "systemMessage": format!("locrin: {e:#}; not blocking the stop") })))
        }
        // A clean verdict closes the loop, so the budget is returned: a session
        // that fixes what it broke and later breaks something else gets the full
        // three rounds again.
        Outcome::Done(Ok(v)) if v.blocking == 0 => {
            session.rounds = 0;
            save(&session);
            emit(None)
        }
        Outcome::Done(Ok(v)) => {
            session.rounds += 1;
            save(&session);
            if session.rounds <= MAX_ROUNDS {
                let capped = v.capped(locrin_reporters::agent::CAP);
                emit(Some(json!({
                    "decision": "block",
                    "reason": format!(
                        "{}\nRound {} of {}: fix the blocking findings above, then stop again.",
                        text::feedback(&capped),
                        session.rounds,
                        MAX_ROUNDS
                    )
                })))
            } else {
                // The findings are still there and the agent is not being sent
                // back for them, so this one goes to the person, with the
                // command that shows them the same view the hook had.
                emit(Some(json!({
                    "systemMessage": format!(
                        "locrin: {} blocking finding(s) remain after {} rounds; the agent was not sent back \
                         again. Run `locrin check --base HEAD` to see them.",
                        v.blocking, MAX_ROUNDS
                    )
                })))
            }
        }
    }
}

/// Checks the files staged for the next commit and returns the verdict's exit
/// code, so git lets the commit through or stops it.
///
/// The scope is the index, not the working tree: a commit carries what was
/// staged, and half-finished work beside it is not what the author is asking to
/// record. That is the one thing this hook has that `check --base HEAD` does
/// not.
///
/// Two things set it apart from the agent hooks. There is no watchdog: a commit
/// is not an agent turn, nobody is waiting on a two-second budget, and the person
/// asked for the gate. And the exit code is the verdict rather than always 0, so
/// an engine error propagates to `main`'s exit 2 and a broken engine stops the
/// commit rather than waving it through as a pass. Someone who disagrees with the
/// gate has `git commit --no-verify`, which is a deliberate act and leaves a
/// trace in the shell history; a hook that silently passed would leave none.
pub fn pre_commit(root: &Path) -> anyhow::Result<i32> {
    let root = canonical_root(root);
    let staged = crate::git::staged_files(&root)?;
    let paths: Vec<PathBuf> = staged
        .into_iter()
        .map(|rel| root.join(rel))
        // A file staged and then removed from the working tree is still a staged
        // change, and there is nothing left on disk to read: `explicit_files`
        // refuses a named path that is not there, which would turn a legitimate
        // commit into an engine error.
        .filter(|p| p.is_file())
        .collect();
    if paths.is_empty() {
        // `git commit` runs this before it notices the stage is empty, so this
        // is a normal thing to hit and not a complaint.
        eprintln!("locrin: nothing staged to check");
        return Ok(0);
    }
    let opts = run::Options {
        root,
        paths,
        changed_only: false,
        json: false,
        // A commit is as interactive as an edit is: nobody waits on a network
        // fetch to find out whether their commit is allowed.
        offline: true,
        diff: None,
    };
    let verdict = run::check(&opts)?;
    print!("{}", locrin_reporters::terminal::render(&verdict));
    Ok(verdict.exit_code())
}

/// Records the round, or says why it could not.
///
/// A counter that cannot be written is not worth failing a stop over: the check
/// still ran and its answer still reaches the agent. What it costs is the memory
/// of this round, so it is reported rather than swallowed.
fn save(session: &Session) {
    if let Err(e) = session.save() {
        eprintln!("locrin: could not record the stop round: {e:#}");
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

    /// The exit code is the whole point: 0 is what tells Claude Code the hook
    /// had nothing to say, and anything else is read as the hook failing. What
    /// it prints on stdout is pinned by the end-to-end tests, which can see it.
    #[test]
    fn a_guarded_hook_that_panics_still_exits_zero() {
        // The panic report this prints on stderr belongs to the test harness,
        // not to the binary, which installs a silent panic hook of its own.
        assert_eq!(guarded(|| panic!("engine bug")), 0);
        assert_eq!(guarded(|| 0), 0);
    }

    #[test]
    fn the_watchdog_reports_a_worker_that_panics_as_a_crash() {
        // The panic report this prints on stderr belongs to the test: the
        // binary installs a silent panic hook, the test harness does not.
        let crashed = watchdog(|| -> anyhow::Result<i32> { panic!("engine bug") });
        assert!(matches!(crashed, Outcome::Crashed));
    }

    /// Only a positive number is a budget. An unset or empty variable is
    /// someone who never set one, and a zero is a budget no check can finish
    /// inside, which would turn every hook into a timeout; both fall back to
    /// the default rather than being obeyed.
    #[test]
    fn only_a_positive_number_overrides_the_budget() {
        assert_eq!(budget_from(Some("500")), 500);
        assert_eq!(budget_from(Some(" 10000 ")), 10_000);
        assert_eq!(budget_from(None), HOOK_BUDGET_MS);
        assert_eq!(budget_from(Some("")), HOOK_BUDGET_MS);
        assert_eq!(budget_from(Some("soon")), HOOK_BUDGET_MS);
        assert_eq!(budget_from(Some("-1")), HOOK_BUDGET_MS);
        assert_eq!(budget_from(Some("2.5")), HOOK_BUDGET_MS);
        assert_eq!(budget_from(Some("0")), HOOK_BUDGET_MS);
    }

    /// A sub-second budget rendered as whole seconds reads as no time at all,
    /// and the message it lands in is the one telling a person why their edit
    /// went unchecked.
    #[test]
    fn the_budget_reads_in_the_unit_that_shows_it() {
        assert_eq!(budget_text(HOOK_BUDGET_MS), "2 s");
        assert_eq!(budget_text(1), "1 ms");
        assert_eq!(budget_text(999), "999 ms");
        assert_eq!(budget_text(10_000), "10 s");
    }

    #[test]
    fn the_watchdog_gives_up_on_a_worker_that_outlasts_the_budget() {
        let slow = watchdog(|| {
            std::thread::sleep(Duration::from_millis(HOOK_BUDGET_MS + 500));
            Ok(1)
        });
        assert!(matches!(slow, Outcome::Timeout));
        let Outcome::Done(Ok(quick)) = watchdog(|| Ok(1)) else {
            panic!("a worker that returns at once is Done");
        };
        assert_eq!(quick, 1);
    }
}
