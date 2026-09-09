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

/// A directory target is a scope filter, not a new root: the config's
/// repo-relative excludes still decide which files inside it are eligible.
#[test]
fn config_excludes_apply_to_directory_targets() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("locrin.toml"), "excludes = [\"src/dirty.ts\"]\n").unwrap();
    let out = locrin(dir.path()).args(["check", "src"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("PASS  0 finding(s)"), "{text}");
}

/// The same file named two ways is one file: an unnormalised spelling must not
/// mint a second identity for the finding or a second row in the index.
#[test]
fn explicit_path_is_canonicalised() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let full: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    let out = locrin(dir.path()).args(["check", "src/../src/dirty.ts", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["findings"][0]["file"], "src/dirty.ts");
    assert_eq!(v["findings"][0]["id"], full["findings"][0]["id"]);
}

#[test]
fn unknown_explicit_path_exits_two() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "src/typo.ts"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("error:"), "{err}");
    assert!(err.contains("typo.ts"), "{err}");
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

/// A baseline command reads the repository, it does not observe it. If `accept`
/// recorded the index state it saw, the edit it happened to walk past would look
/// already-seen and the next `check --changed` would miss it entirely.
#[test]
fn baseline_accept_does_not_swallow_a_concurrent_edit() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["rule"] == "leftover-debug" && f["file"] == "src/dirty.ts")
        .expect("the fixture's dirty.ts debug finding")["id"]
        .as_str()
        .unwrap()
        .to_string();

    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  debugger;\n  return 1;\n}\n")
        .unwrap();

    locrin(dir.path()).args(["baseline", "accept", &id, "--reason", "legacy"]).assert().success();

    let out = locrin(dir.path()).args(["check", "--changed"]).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("src/clean.ts"), "{text}");
}

/// The rules named by the entries of the baseline file in `dir`.
fn baseline_rules(dir: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("locrin-baseline.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["entries"].as_array().unwrap().iter().map(|e| e["rule"].as_str().unwrap().to_string()).collect()
}

/// The first run against a repository is the one that writes the baseline, and
/// on a fresh cache (every CI job, every fresh clone) nothing has written the
/// index the graph rules query. `baseline create` has to build that index for
/// itself, or the line in the sand silently omits every graph finding and the
/// next `check` blocks on debt the baseline was supposed to hold.
#[test]
fn baseline_create_on_a_fresh_cache_captures_dead_exports() {
    let dir = copy_fixture();
    // extra.ts is imported, so it is not an orphan file, but nothing imports the
    // name it exports.
    std::fs::write(dir.path().join("src/extra.ts"), "export const unused = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport \"./extra\";\nexport const total = ok() + bad();\n",
    )
    .unwrap();

    locrin(dir.path()).args(["baseline", "create"]).assert().success();
    let rules = baseline_rules(dir.path());
    assert!(rules.iter().any(|r| r == "dead-export"), "the baseline holds no dead-export entry: {rules:?}");

    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let dead: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert!(dead.is_empty(), "the baseline should have suppressed these: {dead:?} in {v}");
}

/// The same on the rule the operator declared themselves: a boundary the
/// repository already violates belongs in the first baseline, and a `check`
/// straight after `baseline create` has nothing left to block on.
#[test]
fn baseline_create_on_a_fresh_cache_captures_boundary_violations() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[[boundaries]]\nname = \"index stays off dirty\"\nfrom = \"src/index.ts\"\nforbid = [\"src/dirty.ts\"]\n",
    )
    .unwrap();

    locrin(dir.path()).args(["baseline", "create"]).assert().success();
    let rules = baseline_rules(dir.path());
    assert!(rules.iter().any(|r| r == "boundary-violation"), "the baseline holds no boundary entry: {rules:?}");

    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{text}");
}

/// The index database the runs against `dir` wrote, found under the cache
/// directory the test pinned with `LOCRIN_CACHE_DIR`.
fn index_db(dir: &std::path::Path) -> PathBuf {
    walkdir(&dir.join(".cache"))
        .into_iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("index.db"))
        .expect("the run should have written an index database")
}

/// How the index currently resolves the `src/a.ts` -> `./b` import edge.
fn edge_resolution(dir: &std::path::Path) -> Option<String> {
    use rusqlite::OptionalExtension;
    let conn = rusqlite::Connection::open(index_db(dir)).unwrap();
    conn.query_row("SELECT resolution FROM edges WHERE from_rel = 'src/a.ts' AND specifier = './b'", [], |r| r.get(0))
        .optional()
        .unwrap()
}

/// Deleting a file marks its importers' edges unresolved, and those importers do
/// not change, so nothing would re-index them when the file comes back. A run
/// that indexes a file the index has never seen has to repair them, or every
/// graph rule would keep calling the restored file dead.
#[test]
fn a_restored_file_gets_its_incoming_edges_resolved_again() {
    let dir = copy_fixture();
    let b_source = "export function helper(): number {\n  return 1;\n}\n";
    std::fs::write(dir.path().join("src/a.ts"), "import { helper } from \"./b\";\nexport const v = helper();\n")
        .unwrap();
    std::fs::write(dir.path().join("src/b.ts"), b_source).unwrap();

    locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(edge_resolution(dir.path()).as_deref(), Some("resolved"));

    std::fs::remove_file(dir.path().join("src/b.ts")).unwrap();
    locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(edge_resolution(dir.path()).as_deref(), Some("unresolved"), "a departed target unresolves the edge");

    std::fs::write(dir.path().join("src/b.ts"), b_source).unwrap();
    locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(
        edge_resolution(dir.path()).as_deref(),
        Some("resolved"),
        "the unchanged importer must be re-indexed once the target is back"
    );
}

#[test]
fn engine_error_exits_two() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("locrin.toml"), "excludes = [").unwrap();
    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("error:"));
}

/// Removing one import of an export makes that export dead in a file the edit
/// never touched. A named-path check reports on the named file only, so the dead
/// export shows up on a full check; part B widens narrowed checks to the
/// neighbours of what changed. The importer keeps a second name from the same
/// file, because a file that loses its last incoming edge is `dead-file`'s to
/// report and `dead-export` stays quiet about it.
#[test]
fn removing_an_import_surfaces_a_dead_export_on_a_full_check() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("src/lib.ts"),
        "export function kept(): number {\n  return 1;\n}\nexport function dropped(): number {\n  return 2;\n}\n",
    )
    .unwrap();
    let index_with_both = "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { kept, dropped } from \"./lib\";\nexport const total = ok() + bad() + kept() + dropped();\n";
    std::fs::write(dir.path().join("src/index.ts"), index_with_both).unwrap();
    locrin(dir.path()).arg("check").output().unwrap();

    let index_without_dropped = "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { kept } from \"./lib\";\nexport const total = ok() + bad() + kept();\n";
    std::fs::write(dir.path().join("src/index.ts"), index_without_dropped).unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let dead: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(dead, vec!["src/lib.ts"], "{v}");
    let evidence = v["findings"].as_array().unwrap().iter().find(|f| f["rule"] == "dead-export").unwrap()["evidence"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(evidence.contains("`dropped`"), "{evidence}");
}

#[test]
fn unused_import_blocks_and_names_the_binding() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nexport const total = ok();\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "src/index.ts"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("unused-import"), "{text}");
    assert!(text.contains("`bad` is imported from \"./dirty\" but never used"), "{text}");
}

#[test]
fn boundaries_from_config_block() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[[boundaries]]\nname = \"index stays off dirty\"\nfrom = \"src/index.ts\"\nforbid = [\"src/dirty.ts\"]\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rules: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["rule"].as_str().unwrap()).collect();
    assert!(rules.contains(&"boundary-violation"), "{v}");
    assert_eq!(v["status"], "block");
}

/// `dead-file` ships off, so the repository turns it on first. What the test is
/// about is the verdict once it is on: an orphan is advisory and does not block.
#[test]
fn a_new_orphan_file_is_advisory_not_blocking() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("locrin.toml"), "[rules.dead-file]\nenabled = true\n").unwrap();
    locrin(dir.path()).args(["baseline", "create"]).assert().success();
    std::fs::write(dir.path().join("src/orphan.ts"), "export const orphan = 1;\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "advisory", "{v}");
    assert_eq!(v["findings"][0]["rule"], "dead-file");
    assert_eq!(v["findings"][0]["file"], "src/orphan.ts");
}

/// The stored content hash for one file, read straight from the index.
fn stored_hash(dir: &std::path::Path, rel: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    let conn = rusqlite::Connection::open(index_db(dir)).unwrap();
    conn.query_row("SELECT content_hash FROM files WHERE rel = ?1", [rel], |r| r.get(0)).optional().unwrap()
}

/// The stored size and modification time for one file, read straight from the
/// index.
fn stored_stat(dir: &std::path::Path, rel: &str) -> Option<(i64, i64)> {
    use rusqlite::OptionalExtension;
    let conn = rusqlite::Connection::open(index_db(dir)).unwrap();
    conn.query_row("SELECT size, mtime FROM files WHERE rel = ?1", [rel], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .unwrap()
}

/// A file touched without being edited is unchanged, and the content hash is
/// what says so. The run also stores the stat it took on the way, so the next
/// run takes the shortcut instead of reading and hashing that file all over
/// again. A checkout, a formatter or a stash pop rewrites hundreds of unchanged
/// files at once, and without this every later run would pay for all of them,
/// forever.
#[test]
fn a_touched_file_gets_its_stored_stat_refreshed() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();

    let touched = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let path = dir.path().join("src/clean.ts");
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(touched).unwrap();
    drop(file);

    let out = locrin(dir.path()).arg("scan").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("0 changed"), "the bytes did not change, but the run said: {text}");
    assert_eq!(
        stored_stat(dir.path(), "src/clean.ts").map(|s| s.1),
        Some(1_000_000_000_000_000_000),
        "the stat the run took has to replace the stale one, in nanoseconds"
    );
}

/// The shortcut is a stat match and deliberately nothing more: a file whose size
/// and modification time still equal the ones the index recorded is skipped
/// without being opened, even when its bytes have changed underneath. That is
/// what keeps a narrowed run from reading the whole repository, and the hole it
/// leaves (a rewrite that preserves both the length and the modification time to
/// the nanosecond) is not something an editor, a compiler or a checkout does.
#[test]
fn a_file_whose_stat_still_matches_is_skipped_without_being_read() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let before = stored_hash(dir.path(), "src/clean.ts").expect("scan indexes clean.ts");

    let path = dir.path().join("src/clean.ts");
    let original = std::fs::read_to_string(&path).unwrap();
    let rewritten = original.replace("return 1;", "return 2;");
    assert_eq!(rewritten.len(), original.len(), "the rewrite has to keep the file's length");
    assert_ne!(rewritten, original, "the rewrite has to change the bytes");
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, &rewritten).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();

    let out = locrin(dir.path()).arg("scan").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("0 changed"), "a file whose stat matches is not read at all, but the run said: {text}");
    assert_eq!(
        stored_hash(dir.path(), "src/clean.ts").as_deref(),
        Some(before.as_str()),
        "a file that was never read cannot have been re-hashed"
    );
}

/// The stat recorded for a file belongs to the bytes that were read, so a save
/// landing between the read and the record leaves the index holding a stat older
/// than the file's real one. That disagreement is what makes the next run read
/// the file again instead of trusting the shortcut and serving stale symbols,
/// edges and findings until somebody edits the file.
#[test]
fn a_stored_stat_older_than_the_file_forces_a_re_read() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let before = stored_hash(dir.path(), "src/clean.ts").expect("scan indexes clean.ts");

    // New bytes, same length, same modification time: to a later run the file on
    // disk looks exactly as it did to the scan, and only the index knows better.
    let path = dir.path().join("src/clean.ts");
    let original = std::fs::read_to_string(&path).unwrap();
    let rewritten = original.replace("return 1;", "return 2;");
    assert_eq!(rewritten.len(), original.len(), "the rewrite has to keep the file's length");
    assert_ne!(rewritten, original, "the rewrite has to change the bytes");
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, &rewritten).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
    // The state a run leaves behind when a save landed between its stat and its
    // record: the stat it stored is older than the file's real one.
    let conn = rusqlite::Connection::open(index_db(dir.path())).unwrap();
    conn.execute("UPDATE files SET mtime = mtime - 1 WHERE rel = 'src/clean.ts'", []).unwrap();
    drop(conn);

    let out = locrin(dir.path()).arg("scan").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("1 changed"), "a stat the index disagrees with has to be read again, but: {text}");
    assert_ne!(
        stored_hash(dir.path(), "src/clean.ts").as_deref(),
        Some(before.as_str()),
        "the re-read has to record what the file says now"
    );
}

/// A narrowed run skips a file whose size and modification time still match the
/// ones it recorded, but the stat is only a shortcut: when it disagrees the run
/// still reads and hashes the file. So a file that was touched without being
/// edited is reported as unchanged, exactly as it was before the shortcut
/// existed, and its stored hash is left alone.
#[test]
fn a_touched_but_unedited_file_is_still_unchanged() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let before = stored_hash(dir.path(), "src/clean.ts").expect("scan indexes clean.ts");

    let path = dir.path().join("src/clean.ts");
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000)).unwrap();
    drop(file);

    let out = locrin(dir.path()).arg("scan").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("0 changed"), "the hash has to overrule the stat, but the run said: {text}");
    assert_eq!(stored_hash(dir.path(), "src/clean.ts").as_deref(), Some(before.as_str()));
}

/// The dead export lives in a file the edit never touched, and after the edit
/// nothing connects the two: `src/index.ts` no longer imports `src/lib.ts` at
/// all. A `--changed` run must still report it, because the changed file's *old*
/// edge reached it, which is exactly what made the export dead. `src/lib.ts`
/// keeps its other importer, so this is `dead-export` and not `dead-file`.
#[test]
fn changed_only_reports_graph_findings_on_neighbours() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("src/lib.ts"),
        "export function kept(): number {\n  return 1;\n}\nexport function dropped(): number {\n  return 2;\n}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/other.ts"), "import { kept } from \"./lib\";\nexport const a = kept();\n")
        .unwrap();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { dropped } from \"./lib\";\nimport { a } from \"./other\";\nexport const total = ok() + bad() + dropped() + a;\n",
    )
    .unwrap();
    locrin(dir.path()).arg("check").output().unwrap();

    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { a } from \"./other\";\nexport const total = ok() + bad() + a;\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--changed", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["src/lib.ts"], "{v}");
    // The file rule finding on dirty.ts (console.log) is NOT in a --changed run: dirty.ts did not change.
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "leftover-debug"), "{v}");
}

/// After a scan, a full check must not re-parse anything: the cache answers for
/// every unchanged file. Observable through the index: `scan` then `check` leaves
/// exactly one cache row per (file, file rule), and a `check` after an edit to
/// one file rewrites only that file's rows.
#[test]
fn full_check_serves_unchanged_files_from_the_cache() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let db = index_db(dir.path());
    let count = |sql: &str| -> i64 {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    };
    let per_file = count("SELECT count(DISTINCT rule) FROM findings_cache WHERE rel = 'src/clean.ts'");
    assert!(per_file >= 5, "scan warms every file rule, got {per_file}");
    let before = count("SELECT count(*) FROM findings_cache");

    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("BLOCK  2 finding(s)"));
    assert_eq!(count("SELECT count(*) FROM findings_cache"), before, "a warm check adds no rows");

    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  debugger;\n  return 1;\n}\n")
        .unwrap();
    locrin(dir.path()).arg("check").output().unwrap();
    let stale = count(
        "SELECT count(*) FROM findings_cache c JOIN files f ON f.rel = c.rel WHERE f.content_hash <> c.content_hash",
    );
    assert_eq!(stale, 0, "every cache row must carry its file's current hash");
}

/// A severity override changes what the cache may serve.
#[test]
fn config_change_invalidates_the_cache() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("check").output().unwrap();
    std::fs::write(dir.path().join("locrin.toml"), "[rules.leftover-debug]\nseverity = \"low\"\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let debug = v["findings"].as_array().unwrap().iter().find(|f| f["rule"] == "leftover-debug").unwrap();
    assert_eq!(debug["severity"], "low", "{v}");
}

/// The repair pass reads an importer the cache had already answered for, and the
/// rules then produce that file's findings a second time. Whatever the cache
/// served for a file the run ended up parsing has to give way, or one restored
/// target makes its importer report every finding twice.
#[test]
fn a_repaired_importer_is_not_reported_twice() {
    let dir = copy_fixture();
    let b_source = "export function helper(): number {\n  return 1;\n}\n";
    std::fs::write(
        dir.path().join("src/a.ts"),
        "import { helper } from \"./b\";\nconsole.log(helper());\nexport const v = 1;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/b.ts"), b_source).unwrap();
    locrin(dir.path()).arg("check").output().unwrap();
    std::fs::remove_file(dir.path().join("src/b.ts")).unwrap();
    locrin(dir.path()).arg("check").output().unwrap();

    // b.ts is back, so a.ts is re-recorded to resolve its edge again even though
    // the cache had already answered for it.
    std::fs::write(dir.path().join("src/b.ts"), b_source).unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let n = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["file"] == "src/a.ts" && f["rule"] == "leftover-debug")
        .count();
    assert_eq!(n, 1, "{v}");
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn base_sees_the_working_tree_and_since_sees_only_commits() {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);

    // dirty.ts blocks on a full check but is untouched by this diff.
    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  debugger;\n  return 1;\n}\n")
        .unwrap();
    let out = locrin(dir.path()).args(["check", "--base", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert_eq!(files, vec!["src/clean.ts"], "{v}");
    assert_eq!(v["status"], "block");

    let out = locrin(dir.path()).args(["check", "--since", &base]).output().unwrap();
    assert!(
        String::from_utf8(out.stdout).unwrap().starts_with("PASS  0 finding(s)"),
        "uncommitted work is invisible to --since"
    );

    git(dir.path(), &["commit", "-qam", "edit"]);
    let out = locrin(dir.path()).args(["check", "--since", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["findings"][0]["file"], "src/clean.ts", "{v}");

    // An untracked file is part of the working tree view.
    std::fs::write(dir.path().join("src/new.ts"), "export const n = 1;\nconsole.log(n);\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--base", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["findings"].as_array().unwrap().iter().any(|f| f["file"] == "src/new.ts"), "{v}");
}

#[test]
fn a_bad_ref_is_an_engine_error() {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let out = locrin(dir.path()).args(["check", "--base", "no-such-ref"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("error:"), "{err}");
    assert!(err.contains("no-such-ref"), "{err}");
}

/// A file leaving the repository is a change, and the finding it causes lands in
/// a file the run never touched: the departed file was the last importer of an
/// export, so that export is dead now. Nothing about the surviving files changed,
/// so a `--changed` run has an empty scope and only the deleted file's old edges
/// connect it to the answer.
#[test]
fn changed_only_reports_a_dead_export_caused_by_a_deletion() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("src/lib.ts"),
        "export function kept(): number {\n  return 1;\n}\nexport function dropped(): number {\n  return 2;\n}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/one.ts"), "import { kept } from \"./lib\";\nexport const a = kept();\n")
        .unwrap();
    std::fs::write(dir.path().join("src/two.ts"), "import { dropped } from \"./lib\";\nexport const b = dropped();\n")
        .unwrap();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { a } from \"./one\";\nexport const total = ok() + bad() + a;\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "dead-export"), "{v}");

    // two.ts was the only consumer of `dropped`, and it is gone.
    std::fs::remove_file(dir.path().join("src/two.ts")).unwrap();
    let out = locrin(dir.path()).args(["check", "--changed", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let dead: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(dead, vec!["src/lib.ts"], "{v}");
}

#[test]
fn sarif_output_lists_every_rule_and_every_finding() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--sarif"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    let run = &doc["runs"][0];
    assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 8);
    assert_eq!(run["results"].as_array().unwrap().len(), 2);
    assert_eq!(run["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"], "src/dirty.ts");
}

/// Named paths and a diff scope each decide which files the run sees. Taking
/// both would mean one silently winning, so clap refuses the invocation with
/// its usage exit code instead.
#[test]
fn named_paths_and_a_diff_scope_are_a_usage_error() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "src/dirty.ts", "--base", "main"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot be used with '--base"), "{err}");

    let out = locrin(dir.path()).args(["check", "src/dirty.ts", "--since", "main"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot be used with '--since"), "{err}");
}

/// The four files the two tests below share: `two.ts` is the only consumer of
/// `dropped`, so deleting it makes that export dead in `lib.ts`, a file nothing
/// else in the run touches.
fn deletion_shaped_repo(dir: &std::path::Path) {
    std::fs::write(
        dir.join("src/lib.ts"),
        "export function kept(): number {\n  return 1;\n}\nexport function dropped(): number {\n  return 2;\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/one.ts"), "import { kept } from \"./lib\";\nexport const a = kept();\n").unwrap();
    std::fs::write(dir.join("src/two.ts"), "import { dropped } from \"./lib\";\nexport const b = dropped();\n")
        .unwrap();
    std::fs::write(
        dir.join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nimport { a } from \"./one\";\nexport const total = ok() + bad() + a;\n",
    )
    .unwrap();
}

/// A diff scope's verdict is a function of the tree and the ref, and of nothing
/// else. The index's watermark moves the moment a run consumes a deletion, so a
/// `--base` scope that widened itself with that watermark would report a dead
/// export on the first run and pass on an identical second one: the same command
/// on the same tree against the same ref, two different answers.
#[test]
fn a_diff_scope_gives_the_same_verdict_twice() {
    let dir = copy_fixture();
    deletion_shaped_repo(dir.path());
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    // Warm the index while two.ts is still on disk, so the first --base run
    // below is the one that sees it go.
    locrin(dir.path()).args(["check", "--json"]).output().unwrap();

    std::fs::remove_file(dir.path().join("src/two.ts")).unwrap();
    let findings = |dir: &std::path::Path| -> serde_json::Value {
        let out = locrin(dir).args(["check", "--base", "HEAD", "--json"]).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["findings"].clone()
    };
    let first = findings(dir.path());
    let second = findings(dir.path());
    assert_eq!(first, second, "the same --base run twice must answer the same");
    // The deletion is not in the diff scope (the file is gone from the working
    // tree), so neither run reports the export it killed.
    assert_eq!(first.as_array().unwrap().len(), 0, "{first}");
}

/// `--since` is the deployment gate: it answers for the commits in the range and
/// for what their edges reach. An uncommitted deletion is outside that range, so
/// nothing it causes may appear, however recently the index learned about it.
#[test]
fn since_reports_nothing_from_an_uncommitted_deletion() {
    let dir = copy_fixture();
    deletion_shaped_repo(dir.path());
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    locrin(dir.path()).args(["check", "--json"]).output().unwrap();

    // One commit in the range, on a file whose edges never reach lib.ts.
    std::fs::write(dir.path().join("src/clean.ts"), "// touched\nexport function ok(): number {\n  return 1;\n}\n")
        .unwrap();
    git(dir.path(), &["commit", "-qam", "edit"]);
    std::fs::remove_file(dir.path().join("src/two.ts")).unwrap();

    let out = locrin(dir.path()).args(["check", "--since", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(!files.contains(&"src/lib.ts"), "{v}");
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "dead-export"), "{v}");
}

/// The rest of the check flags that cannot be combined. `--sarif` and `--json`
/// are two output formats for one stdout; `--changed` and `--base` are two
/// answers to which files the run sees.
#[test]
fn conflicting_check_flags_are_a_usage_error() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--sarif", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot be used with"), "{err}");
    assert!(err.contains("--json"), "{err}");

    let out = locrin(dir.path()).args(["check", "--changed", "--base", "main"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot be used with"), "{err}");
    assert!(err.contains("--base"), "{err}");
}
