use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;

fn named_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn copy_fixture() -> tempfile::TempDir {
    copy_named_fixture("repo")
}

/// A writable copy of one fixture tree. Tests edit the copy (a config file, a
/// source file) and must not edit the fixture the next test reads.
fn copy_named_fixture(name: &str) -> tempfile::TempDir {
    let src = named_fixture(name);
    let dir = tempfile::tempdir().unwrap();
    for entry in walkdir(&src) {
        let rel = entry.strip_prefix(&src).unwrap();
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
/// every unchanged file. Row counts alone would pass on an engine that ignored
/// the cache and re-derived the same answers, so the proof is a probe: a finding
/// planted in `clean.ts`'s cached row, which no rule could ever produce, appears
/// in the verdict, and disappears the moment that row's content hash stops
/// matching the file.
#[test]
fn full_check_serves_unchanged_files_from_the_cache() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let db = index_db(dir.path());
    let count = |sql: &str| -> i64 {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    };
    let execute = |sql: &str| {
        let c = rusqlite::Connection::open(&db).unwrap();
        assert_eq!(c.execute(sql, []).unwrap(), 1, "the probe must land on exactly one row: {sql}");
    };
    let evidence = |args: &[&str]| -> Vec<String> {
        let out = locrin(dir.path()).args(args).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["findings"].as_array().unwrap().iter().map(|f| f["evidence"].as_str().unwrap().to_string()).collect()
    };
    let per_file = count("SELECT count(DISTINCT rule) FROM findings_cache WHERE rel = 'src/clean.ts'");
    assert!(per_file >= 5, "scan warms every file rule, got {per_file}");
    let before = count("SELECT count(*) FROM findings_cache");

    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("BLOCK  2 finding(s)"));
    assert_eq!(count("SELECT count(*) FROM findings_cache"), before, "a warm check adds no rows");

    // `clean.ts` is clean, so nothing but the cache can put this in a verdict.
    let planted = r#"[{"id":"planted000000001","rule":"leftover-debug","category":"erosion","severity":"high",
        "confidence":"high","file":"src/clean.ts","span":{"start_line":1,"start_col":0,"end_line":1,"end_col":1},
        "evidence":"planted","fix":"planted","related":[],"owasp":null,"cwe":null}]"#;
    execute(&format!(
        "UPDATE findings_cache SET findings = '{}' WHERE rel = 'src/clean.ts' AND rule = 'leftover-debug'",
        planted.replace('\n', "").replace("        ", "")
    ));
    let found = evidence(&["check", "--json"]);
    assert!(found.iter().any(|e| e == "planted"), "an unchanged file's findings come from the cache: {found:?}");

    // A row whose hash no longer describes the file on disk is not that file's
    // answer, so the file is read and the planted finding goes.
    execute("UPDATE findings_cache SET content_hash = 'x' WHERE rel = 'src/clean.ts' AND rule = 'leftover-debug'");
    let found = evidence(&["check", "--json"]);
    assert!(!found.iter().any(|e| e == "planted"), "a stale row must never be served: {found:?}");

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

/// A ref is data the operator hands the tool, and git reads an argument that
/// begins with `-` as one of its own options. `--since=--output=<path>` used to
/// reach `git diff` as `--output`, which wrote the diff to that path and let the
/// check print PASS: an attacker-controlled ref in a CI configuration could
/// write a file anywhere the runner could. The ref is refused before any git
/// process starts, so nothing is written and the message names what was refused.
#[test]
fn a_ref_that_looks_like_an_option_is_refused() {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let planted = dir.path().join("planted.diff");
    let arg = format!("--since=--output={}", planted.display());
    let out = locrin(dir.path()).args(["check", arg.as_str()]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stdout));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("error:"), "{err}");
    assert!(err.contains("--output="), "the message names the ref it refused: {err}");
    assert!(!planted.exists(), "git must never have seen the ref as an option");
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
    assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 21);
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

    // Named paths and `--changed` are two answers to which files the run sees,
    // exactly as named paths and a diff scope are. The paths used to win in
    // silence, so a hook that named a file and asked for `--changed` got a
    // narrower run than it read the flag as asking for.
    let out = locrin(dir.path()).args(["check", "src/dirty.ts", "--changed"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot be used with"), "{err}");
    assert!(err.contains("--changed"), "{err}");
}

/// The engine's copy of the module under test. A binary crate has no library for
/// an integration test to link against, so the source is compiled a second time
/// here; only `show_at` is exercised, and `base_rev` is covered through the
/// `--base` and `--since` tests above, which run every diff through it.
#[path = "../src/git.rs"]
#[allow(dead_code)]
mod git_src;

/// A diff scope has to read the version of a file that is in the base commit,
/// not the one on disk: on a fresh CI clone the index has no memory of any
/// earlier version, and the base commit is the only "before" a pull request has.
///
/// The run happens under `LANG=de_DE.UTF-8` because the "path is not in that
/// revision" answer is decided by matching git's stderr, and both messages
/// matched are translated ones: `show_at` pins `LC_ALL=C` on every git process
/// so a developer or CI runner with a localised environment still gets None
/// rather than an aborted run. Git ships no message catalogs on this machine, so
/// what this proves is that the pinning is harmless, not that it is sufficient;
/// a machine with catalogs installed is what would prove the rest.
#[test]
fn show_at_reads_the_committed_text_and_says_nothing_for_a_path_that_was_not_there() {
    std::env::set_var("LANG", "de_DE.UTF-8");
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    // Line endings are the repository's business, not this test's: without this a
    // machine configured to rewrite them would commit different bytes than were
    // written and the comparison below would be about newlines.
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    let committed = "export const version = 1;\n";
    std::fs::write(dir.path().join("src/committed.ts"), committed).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let root = locrin_core::walk::canonical_root(dir.path());

    // The working tree has moved on; the commit is what show_at answers with.
    std::fs::write(dir.path().join("src/committed.ts"), "export const version = 2;\n").unwrap();
    assert_eq!(git_src::show_at(&root, "HEAD", "src/committed.ts").unwrap().as_deref(), Some(committed));

    // A file added since the commit was not there, which is not an error: it is
    // how the caller learns everything in it is new.
    std::fs::write(dir.path().join("src/added.ts"), "export const added = 1;\n").unwrap();
    assert_eq!(git_src::show_at(&root, "HEAD", "src/added.ts").unwrap(), None);
    assert_eq!(git_src::show_at(&root, "HEAD", "src/never-existed.ts").unwrap(), None);

    // A revision that does not exist is a real failure, not a missing file.
    assert!(git_src::show_at(&root, "no-such-ref", "src/committed.ts").is_err());
    // A revision shaped like a git option never reaches git.
    assert!(git_src::show_at(&root, "--output=planted", "src/committed.ts").is_err());

    std::env::remove_var("LANG");
}

const ACTIVE_TEST: &str =
    "describe(\"rows\", () => {\n  it(\"renders a row\", () => {\n    expect(1).toBe(1);\n  });\n});\n";
const SKIPPED_TEST: &str =
    "describe(\"rows\", () => {\n  it.skip(\"renders a row\", () => {\n    expect(1).toBe(1);\n  });\n});\n";
/// Still skipped, edited again, and now carrying a `console.log`. The debug call
/// is the positive control for every leg that asserts no newly-skipped finding:
/// "no finding" is also what an empty scope produces, so a leg that only asserts
/// the absence would pass if the file had silently dropped out of the run. The
/// `leftover-debug` finding proves the file was in scope and the rule stayed
/// quiet on purpose.
const SKIPPED_TEST_EDITED: &str = "// still skipped, and now edited again\nconsole.log(\"noise\");\ndescribe(\"rows\", () => {\n  it.skip(\"renders a row\", () => {\n    expect(1).toBe(1);\n  });\n});\n";
/// The same two versions carrying the `console.log` from the start, for a leg
/// whose every run needs the positive control rather than only its last one.
const ACTIVE_TEST_WITH_DEBUG: &str = "console.log(\"noise\");\ndescribe(\"rows\", () => {\n  it(\"renders a row\", () => {\n    expect(1).toBe(1);\n  });\n});\n";
const SKIPPED_TEST_WITH_DEBUG: &str = "console.log(\"noise\");\ndescribe(\"rows\", () => {\n  it.skip(\"renders a row\", () => {\n    expect(1).toBe(1);\n  });\n});\n";

/// Every `test-newly-skipped` finding in a `--json` payload, as (file, evidence).
fn newly_skipped(v: &serde_json::Value) -> Vec<(String, String)> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "test-newly-skipped")
        .map(|f| (f["file"].as_str().unwrap().to_string(), f["evidence"].as_str().unwrap().to_string()))
        .collect()
}

/// Whether the payload reports the planted `console.log` in `rel`. See
/// [`SKIPPED_TEST_EDITED`].
fn debug_reported(v: &serde_json::Value, rel: &str) -> bool {
    v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "leftover-debug" && f["file"] == rel)
}

/// The index is the source of "previous" for an ordinary run: what it remembered
/// about a file is the version the edit replaced. A test active at the last run
/// and skipped now is the finding; once the skip is in the index, the next edit
/// to the same file does not report it again.
#[test]
fn changed_reports_a_test_this_edit_skipped_and_not_one_the_index_already_knew() {
    let dir = copy_fixture();
    let test_file = dir.path().join("src/a.test.ts");
    std::fs::write(&test_file, ACTIVE_TEST).unwrap();
    // The run that puts the active version into the index; it is the "previous"
    // every later run in this test compares against.
    locrin(dir.path()).arg("check").output().unwrap();

    std::fs::write(&test_file, SKIPPED_TEST).unwrap();
    let out = locrin(dir.path()).args(["check", "--changed", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        newly_skipped(&v),
        vec![("src/a.test.ts".to_string(), "test \"renders a row\" is skipped (newly)".to_string())],
        "{v}"
    );

    // A second edit that keeps the skip. The file changed, so it is re-parsed
    // rather than served from the cache, and the index now remembers the skip:
    // the decision is no longer this change's, so the rule says nothing.
    std::fs::write(&test_file, SKIPPED_TEST_EDITED).unwrap();
    let out = locrin(dir.path()).args(["check", "--changed", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(debug_reported(&v, "src/a.test.ts"), "the leg has to prove the file was in scope: {v}");
    assert!(newly_skipped(&v).is_empty(), "a skip the index already knew is not new: {v}");
}

/// A named path is a scope like `--changed`, and the file it names is usually
/// unchanged: the developer is asking about a file, not reporting an edit. Such a
/// file is parsed for the rules without being re-recorded, so the run has to take
/// its previous version from the index anyway. Without that, every `check <path>`
/// over a suite with a legacy skip reports it again, forever.
#[test]
fn a_named_path_does_not_report_a_skip_the_index_already_knew() {
    let dir = copy_fixture();
    let test_file = dir.path().join("src/a.test.ts");
    std::fs::write(
        &test_file,
        "console.log(\"noise\");\ndescribe(\"rows\", () => {\n  it.skip(\"legacy\", () => {\n    expect(1).toBe(1);\n  });\n});\n",
    )
    .unwrap();
    // The whole-repository run that records the skip. It reports it once (the
    // index had never seen the file), and the index remembers it from here on.
    locrin(dir.path()).arg("check").output().unwrap();

    // Twice, because the first named-path run must not be the thing that teaches
    // the index: the file is unchanged, so nothing about it moves between them.
    for run in 1..=2 {
        let out = locrin(dir.path()).args(["check", "src/a.test.ts", "--json"]).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        // The planted `console.log` is the positive control: "no newly-skipped
        // finding" is also what an empty scope produces, so without it this leg
        // would pass if the named path had never reached the file at all.
        assert!(debug_reported(&v, "src/a.test.ts"), "named-path run {run} did not reach the file: {v}");
        assert!(newly_skipped(&v).is_empty(), "named-path run {run} reported a skip the index knew: {v}");
    }
}

/// The lifetime of a newly-skipped finding, which the rule's module doc
/// describes and nothing else pins: reported once, then served from the findings
/// cache until the file is next parsed, and gone from there on.
///
/// A named path is one of the three things that parse an unchanged file (an edit
/// and a cache miss after a config change are the others), so it is what this
/// leg uses to reach that point without editing the file: the named-path run
/// re-runs the rules over the file, and this time the index's record of the skip
/// is the file's previous version, so it writes cache rows with no finding in
/// them and the whole-repository run after it serves those.
#[test]
fn a_newly_skipped_finding_is_served_from_the_cache_until_the_file_is_parsed_again() {
    let dir = copy_fixture();
    let test_file = dir.path().join("src/a.test.ts");
    std::fs::write(&test_file, ACTIVE_TEST_WITH_DEBUG).unwrap();
    // The run that makes the active version the "previous" the next one compares
    // against.
    locrin(dir.path()).arg("check").output().unwrap();

    std::fs::write(&test_file, SKIPPED_TEST_WITH_DEBUG).unwrap();
    let out = locrin(dir.path()).arg("check").args(["--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        newly_skipped(&v),
        vec![("src/a.test.ts".to_string(), "test \"renders a row\" is skipped (newly)".to_string())],
        "the whole-repository run that sees the edit reports the skip: {v}"
    );

    // The named path parses the unchanged file again. The skip is in the index
    // now, so it is no longer new and the rows this run caches say so.
    let out = locrin(dir.path()).args(["check", "src/a.test.ts", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(debug_reported(&v, "src/a.test.ts"), "the named path has to reach the file: {v}");
    assert!(newly_skipped(&v).is_empty(), "a skip the index already knew is not new: {v}");

    // And the whole-repository run after it serves those rows rather than the
    // ones the reporting run wrote, so the finding does not come back.
    let out = locrin(dir.path()).arg("check").args(["--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(debug_reported(&v, "src/a.test.ts"), "the file has to be answered for: {v}");
    assert!(newly_skipped(&v).is_empty(), "the finding outlived the parse that retired it: {v}");
}

/// For a diff scope git overrides the index: the base commit is the "before" a
/// pull request is judged against, and on a fresh CI clone it is the only one
/// there is.
#[test]
fn base_reports_a_test_skipped_since_the_base_commit_and_not_one_the_base_already_skipped() {
    let dir = copy_fixture();
    let test_file = dir.path().join("src/a.test.ts");
    std::fs::write(&test_file, ACTIVE_TEST).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);

    std::fs::write(&test_file, SKIPPED_TEST).unwrap();
    let out = locrin(dir.path()).args(["check", "--base", "HEAD", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        newly_skipped(&v),
        vec![("src/a.test.ts".to_string(), "test \"renders a row\" is skipped (newly)".to_string())],
        "{v}"
    );

    // Committed: the working tree matches HEAD, so the diff is empty and there
    // is nothing to answer for.
    git(dir.path(), &["add", "src"]);
    git(dir.path(), &["commit", "-qm", "skip it"]);
    let out = locrin(dir.path()).args(["check", "--base", "HEAD", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // Nothing at all, not merely no skip: this leg is the one where the scope is
    // genuinely empty, and asserting the whole payload says so is what keeps it
    // from being confused with a leg that suppressed a finding.
    assert!(v["findings"].as_array().unwrap().is_empty(), "an empty diff reports nothing: {v}");

    // And with the file back in the diff for an unrelated edit, the skip is
    // still not reported: it is in the base commit, so it is not this change's.
    std::fs::write(&test_file, SKIPPED_TEST_EDITED).unwrap();
    let out = locrin(dir.path()).args(["check", "--base", "HEAD", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(debug_reported(&v, "src/a.test.ts"), "the leg has to prove the file was in scope: {v}");
    assert!(newly_skipped(&v).is_empty(), "the base commit already skipped it: {v}");
}

/// A lockfileVersion 3 `package-lock.json` with one installed package, written
/// into a scratch copy of the fixture rather than committed beside it: every
/// other test in this file shares that fixture and none of them should start
/// paying for an advisory rule.
const LOCK: &str = r#"{
  "name": "scope-fixture",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {
      "name": "scope-fixture",
      "version": "1.0.0",
      "dependencies": { "left-pad": "^1.3.0" }
    },
    "node_modules/left-pad": {
      "version": "1.3.0",
      "resolved": "https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz"
    }
  }
}
"#;

/// `vulnerable-dependency` answers for the lockfile, and the lockfile is not a
/// source file: no scope's import neighbourhood can ever reach it, so on every
/// scoped run the rule used to pay its whole cost (the lockfile read, the
/// snapshot read, and online the requests) and then have every finding thrown
/// away by the graph filter.
///
/// The contract is now that a scoped run runs the rule only when the lockfile is
/// among the paths the scope itself names, and skips it outright otherwise. The
/// skip is observed through the warning an offline run with no snapshot prints:
/// its absence is proof the rule never ran, because a rule that ran would have
/// printed it.
///
/// Everything here is offline, so no test in this file makes a network call.
#[test]
fn a_scoped_run_reads_the_lockfile_only_when_the_scope_names_it() {
    const SKIPPED: &str = "no cached advisory snapshot; vulnerable-dependency skipped";
    let dir = copy_fixture();
    std::fs::write(dir.path().join("package-lock.json"), LOCK).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let stderr = |args: &[&str]| -> String {
        let out = locrin(dir.path()).args(args).output().unwrap();
        String::from_utf8(out.stderr).unwrap()
    };

    // A whole-repository check answers for the lockfile, so the rule runs.
    let err = stderr(&["check", "--offline"]);
    assert!(err.contains(SKIPPED), "a whole-repository check runs the rule: {err}");

    // A named source file does not, and neither does anything its imports reach.
    let err = stderr(&["check", "src/clean.ts", "--offline"]);
    assert!(!err.contains(SKIPPED), "the rule must not run for a scope that cannot report it: {err}");

    // Naming the lockfile is an instruction to answer for it.
    let err = stderr(&["check", "package-lock.json", "--offline"]);
    assert!(err.contains(SKIPPED), "a scope that names the lockfile runs the rule: {err}");

    // `--changed` takes its scope from the index, which holds source files only,
    // so the lockfile can never be in it however it was edited.
    std::fs::write(dir.path().join("package-lock.json"), LOCK.replace("1.3.0", "1.3.1")).unwrap();
    let err = stderr(&["check", "--changed", "--offline"]);
    assert!(!err.contains(SKIPPED), "--changed cannot see a file that is not indexed: {err}");

    // A diff scope can: git lists every changed file, not only the parsed ones.
    let err = stderr(&["check", "--base", "HEAD", "--offline"]);
    assert!(err.contains(SKIPPED), "the lockfile is in the diff, so the rule runs: {err}");

    // A directory names everything under it, lockfile included: `check .` has to
    // answer for the lockfile exactly as a whole-repository check does.
    let err = stderr(&["check", ".", "--offline"]);
    assert!(err.contains(SKIPPED), "the repository root contains the lockfile: {err}");

    // A directory that does not contain the lockfile still does not name it.
    let err = stderr(&["check", "src", "--offline"]);
    assert!(!err.contains(SKIPPED), "the lockfile is not under src: {err}");
}

/// A polyglot repository installs from more than one registry, and each
/// lockfile is its own package list, its own ecosystem and its own snapshot. All
/// of them are read, each says which file it is about, and an offline run with
/// no snapshot skips the rule for each rather than failing over any of them.
#[test]
fn every_lockfile_in_a_polyglot_repository_is_checked_under_its_own_name() {
    let dir = copy_named_fixture("multilang");
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    for rel in ["composer.lock", "poetry.lock", "requirements.txt"] {
        let expected = format!("warning: {rel}: no cached advisory snapshot; vulnerable-dependency skipped");
        assert!(err.contains(&expected), "{rel} was not checked: {err}");
    }
    // The run finished and reported: a lockfile in a language the engine does
    // not parse is an input, never an error (spec 9).
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(files.iter().all(|f| !f.ends_with(".lock") && !f.ends_with(".txt")), "no advisories offline: {files:?}");
}

/// A migration creating a table nothing locks down, written into a scratch copy
/// rather than committed beside the fixture: every other test in this file
/// shares that fixture and none of them should start paying for the rule that
/// reads migrations.
const MIGRATION: &str = "create table public.orders (id uuid primary key);\n";

/// `supabase-table-without-rls` answers for the SQL migrations, and a migration
/// is not a source file: no scope's import neighbourhood can ever reach one, so
/// on every scoped run the rule used to read every migration and then have all
/// of its findings thrown away by the graph filter.
///
/// The contract is the one `vulnerable-dependency` already has. A scoped run
/// runs the rule only when the raw scope, every existing file the scope names
/// rather than only the source files among them, holds a `.sql` under
/// `supabase/migrations`; and a finding against a file the raw scope names is
/// kept whatever the neighbourhood says.
///
/// Everything here is offline, so no test in this file makes a network call.
#[test]
fn a_scoped_run_reports_a_migration_the_scope_names() {
    let rel = "supabase/migrations/0001_orders.sql";
    let dir = copy_fixture();
    std::fs::create_dir_all(dir.path().join("supabase/migrations")).unwrap();
    std::fs::write(dir.path().join(rel), MIGRATION).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);

    let rls = |args: &[&str]| -> bool {
        let out = locrin(dir.path()).args(args).args(["--json", "--offline"]).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "supabase-table-without-rls" && f["file"] == rel)
    };

    // A whole-repository check answers for every file, migrations included.
    assert!(rls(&["check"]), "a whole-repository check reports the table");

    // Naming the migration is an instruction to answer for it.
    assert!(rls(&["check", rel]), "a scope that names the migration reports the table");

    // Naming a directory names what is under it, at any depth.
    assert!(rls(&["check", "supabase"]), "the migration is under supabase");
    assert!(rls(&["check", "."]), "the repository root contains the migration");

    // A named source file names no migration, so the rule does not run at all.
    assert!(!rls(&["check", "src/index.ts"]), "a scope that cannot report the finding must not produce it");

    // A diff scope can name it: git lists every changed file, not only the
    // parsed ones.
    std::fs::write(dir.path().join(rel), format!("{MIGRATION}-- a later note\n")).unwrap();
    assert!(rls(&["check", "--base", "HEAD"]), "the migration is in the diff");

    // `--changed` takes its scope from the index, which holds source files only,
    // so a migration can never be in it however it was edited.
    assert!(!rls(&["check", "--changed"]), "--changed cannot see a file that is not indexed");
}

/// `baseline create` runs every rule, `vulnerable-dependency` included, so it
/// needs the same promise `check --offline` makes: without it a repository
/// drawing its first line in the sand on a machine with no network waits out
/// the advisory requests' timeouts.
///
/// The flag is observed through the warning an offline run with no snapshot
/// prints. Its presence proves the flag reached the rule, because an online run
/// would have fetched instead.
#[test]
fn baseline_create_takes_offline_and_the_flag_reaches_the_advisory_rule() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("package-lock.json"), LOCK).unwrap();
    let out = locrin(dir.path()).args(["baseline", "create", "--offline"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("no cached advisory snapshot; vulnerable-dependency skipped"),
        "the rule ran offline and said so: {err}"
    );
    assert!(dir.path().join("locrin-baseline.json").exists());
}

/// `--base HEAD` needs a HEAD, and the Stop hook asks this before it builds that
/// scope. The three answers are the three states a directory it runs in can be
/// in, and only the last of them can be diffed against.
#[test]
fn has_head_is_true_only_once_there_is_a_commit() {
    let dir = copy_fixture();
    assert!(!git_src::has_head(dir.path()), "no repository at all");
    git(dir.path(), &["init", "-q"]);
    assert!(!git_src::has_head(dir.path()), "an unborn branch has no commit to diff against");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    assert!(git_src::has_head(dir.path()));
}

/// `--version` is what a bug report is asked for and what an install script
/// checks, and the baseline stamp names the same string, so the flag has to
/// exist and print the crate's own version rather than clap's "unknown".
#[test]
fn version_flag_prints_the_crate_version() {
    let out = Command::cargo_bin("locrin").unwrap().arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(&format!("locrin {}", env!("CARGO_PKG_VERSION"))), "{text}");
}

/// The pull-request view: one marker line the action greps for, then the
/// verdict, so a comment can be found again and edited rather than piled on.
#[test]
fn markdown_output_carries_the_marker_and_blocks() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--markdown", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("<!-- locrin-report -->\n### Locrin: BLOCK"), "{text}");
    assert!(text.contains("`leftover-debug`"));
}

/// Three output formats for one stdout, so clap refuses any pair of them.
#[test]
fn markdown_conflicts_with_json_and_sarif() {
    let dir = copy_fixture();
    for other in ["--json", "--sarif"] {
        let out = locrin(dir.path()).args(["check", "--markdown", other]).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{other}");
    }
}

/// The file is not an output format: it is the complete list, written whatever
/// stdout is showing, so one run can both comment and upload.
#[test]
fn sarif_file_is_written_alongside_any_stdout_format() {
    let dir = copy_fixture();
    let sarif = dir.path().join("out").join("locrin.sarif");
    std::fs::create_dir_all(sarif.parent().unwrap()).unwrap();
    let out =
        locrin(dir.path()).args(["check", "--markdown", "--offline", "--sarif-file"]).arg(&sarif).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("<!-- locrin-report -->"));
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&sarif).unwrap()).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    assert!(doc["runs"][0]["results"].as_array().unwrap().len() >= 2);
}

/// A path the engine cannot write is the engine failing, not the code failing,
/// so it exits 2 and names the file rather than reporting a clean verdict.
#[test]
fn sarif_file_in_a_missing_directory_is_an_engine_error() {
    let dir = copy_fixture();
    let out = locrin(dir.path())
        .args(["check", "--offline", "--sarif-file"])
        .arg(dir.path().join("nope").join("x.sarif"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("x.sarif"));
}

/// The action sets `LOCRIN_RUN_URL` to the workflow run, so the comment can
/// link to the log the SARIF upload came from.
#[test]
fn markdown_details_line_comes_from_locrin_run_url() {
    let dir = copy_fixture();
    let out = locrin(dir.path())
        .args(["check", "--markdown", "--offline"])
        .env("LOCRIN_RUN_URL", "https://example/run/9")
        .output()
        .unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().ends_with("Details: https://example/run/9\n"));
}

/// PHP and Python are read only once `locrin.toml` asks for them. A repository
/// that has not opted in sees nothing from either, and naming such a file on the
/// command line says why rather than reporting it clean.
#[test]
fn php_and_python_are_skipped_until_enabled() {
    let dir = copy_named_fixture("multilang");
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(files.iter().all(|f| f.ends_with(".ts")), "{files:?}");

    let out = locrin(dir.path()).args(["check", "--offline", "src/b.php"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stderr).unwrap().contains("[languages] php = true"));
}

/// The note is one line per language, not one per file: `pre-commit` names every
/// staged file, so a repository with a directory of Python in the stage would
/// have read the same sentence once per file.
#[test]
fn a_skipped_language_is_reported_once_per_run() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(dir.path().join("src/d.php"), "<?php\nfunction d() {}\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--offline", "src/b.php", "src/d.php", "src/c.py"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let err = String::from_utf8(out.stderr).unwrap();
    let php: Vec<&str> = err.lines().filter(|l| l.contains("[languages] php = true")).collect();
    let python: Vec<&str> = err.lines().filter(|l| l.contains("[languages] python = true")).collect();
    assert_eq!(php.len(), 1, "{err}");
    assert!(php[0].contains("src/b.php and 1 more php file skipped"), "{err}");
    assert_eq!(python.len(), 1, "{err}");
    assert!(python[0].contains("src/c.py skipped"), "{err}");
}

/// The other half of the test above: with both languages asked for, the findings
/// carry PHP and Python files. `leftover-debug` is what fires on them, from the
/// `var_dump` and `breakpoint` calls these two fixture files hold.
#[test]
fn enabled_languages_are_reported_on() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(dir.path().join("locrin.toml"), "[languages]\nphp = true\npython = true\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(files.iter().any(|f| f.ends_with("b.php")), "{files:?}");
    assert!(files.iter().any(|f| f.ends_with("c.py")), "{files:?}");
}

/// The findings of one rule, by file.
fn files_of_rule(v: &serde_json::Value, rule: &str) -> Vec<String> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == rule)
        .map(|f| f["file"].as_str().unwrap().to_string())
        .collect()
}

/// A per-language default is the engine's own measurement over a corpus, and
/// `[rules.<id>] languages` is how a repository that has measured it for itself
/// overrules it. `leftover-commented-code` ships off on Python even for a
/// repository that turns the rule on (it ships off everywhere from 0.6.0, so
/// the configs here enable it); naming Python among the rule's languages turns
/// that one pair on, and the Python fixture holds a commented-out block for it
/// to find.
#[test]
fn a_rule_language_override_turns_a_per_language_default_on() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[languages]\npython = true\n\n[rules.leftover-commented-code]\nenabled = true\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let commented = files_of_rule(&v, "leftover-commented-code");
    assert!(!commented.iter().any(|f| f.ends_with("c.py")), "the pair ships off: {commented:?}");

    let dir = copy_named_fixture("multilang");
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[languages]\npython = true\n\n[rules.leftover-commented-code]\nenabled = true\n\
         languages = [\"typescript\", \"tsx\", \"javascript\", \"python\"]\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let commented = files_of_rule(&v, "leftover-commented-code");
    assert!(commented.iter().any(|f| f.ends_with("c.py")), "the override turns it on: {commented:?}");

    // The rule is one rule: the languages it was already on for are still on,
    // because the list named them too.
    let out = locrin(dir.path()).args(["check", "--json", "--offline", "src/b.php"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "PHP is not on this list, so nothing reports on it");
}

/// A language a rule cannot read is a mistake in the config rather than a line
/// that quietly does nothing, and the run names the rule and what it does read.
#[test]
fn a_rule_language_override_a_rule_cannot_read_fails_the_run() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(dir.path().join("locrin.toml"), "[rules.dead-file]\nlanguages = [\"python\"]\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("rules.dead-file.languages"), "{err}");
    assert!(err.contains("typescript"), "the languages the rule does read are named: {err}");

    // And a language the engine does not know at all fails at the config.
    std::fs::write(dir.path().join("locrin.toml"), "[rules.dead-file]\nlanguages = [\"pyhton\"]\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("pyhton"), "{err}");
}

/// `secret-exposed` is locked, so the `rules` table cannot turn it off or lower
/// its severity. `languages` is the key it refuses out loud: narrowing where a
/// locked rule reports is narrowing the lock, and a config that asks for it is
/// told rather than quietly ignored.
#[test]
fn a_languages_override_on_the_locked_rule_fails_the_run() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(dir.path().join("locrin.toml"), "[rules.secret-exposed]\nlanguages = [\"php\"]\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("rules.secret-exposed.languages"), "{err}");
    assert!(err.contains("locked"), "the rule is named and so is the reason: {err}");
}

/// `dead-file` is a graph rule: it reads the index, which holds every file the
/// walk indexed, and its resolver knows JavaScript resolution and nothing else.
/// It declares the JavaScript family, so on a repository with PHP and Python
/// turned on it must stay silent about both however unreachable they look. The
/// two files added here are the shape it would otherwise report: a function and
/// a def that nothing in the repository calls.
#[test]
fn dead_file_never_reports_a_language_it_did_not_declare() {
    let dir = copy_named_fixture("multilang");
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[languages]\nphp = true\npython = true\n\n[rules.dead-file]\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.php"), "<?php\nfunction unused_helper(): int\n{\n    return 1;\n}\n")
        .unwrap();
    std::fs::write(dir.path().join("src/util.py"), "def unused_helper():\n    return 1\n").unwrap();

    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let dead: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-file")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert!(!dead.is_empty(), "the rule has to be running for the assertion below to mean anything: {v}");
    assert!(dead.iter().all(|f| !f.ends_with(".php") && !f.ends_with(".py")), "{dead:?}");
}
