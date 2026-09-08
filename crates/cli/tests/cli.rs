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
