//! Spec section 3.4 targets. Run: cargo test --release -p locrin-cli -- --ignored --nocapture
//! Requires the founder's FastLift checkout at <home>/fasting-app (override with LOCRIN_BENCH_REPO).

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use assert_cmd::prelude::*;

/// These tests measure wall-clock time, so they must never overlap: the libtest
/// harness runs tests on several threads by default and the contention alone
/// inflates a measurement by an order of magnitude. Every benchmark holds this
/// lock for its whole body, which makes the three run one at a time whatever
/// the harness does. Poisoning is ignored: a failing benchmark must not turn
/// the others into lock errors.
static BENCH_LOCK: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    BENCH_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn repo() -> PathBuf {
    PathBuf::from(std::env::var("LOCRIN_BENCH_REPO").unwrap_or_else(|_| "<home>/fasting-app".into()))
}

fn locrin(cache: &std::path::Path) -> Command {
    let dir = repo();
    assert!(
        dir.is_dir(),
        "bench repo {} is not a directory; set LOCRIN_BENCH_REPO to a checkout to benchmark against",
        dir.display()
    );
    let mut c = Command::cargo_bin("locrin").unwrap();
    c.current_dir(dir).env("LOCRIN_CACHE_DIR", cache);
    c
}

#[test]
#[ignore]
fn cold_index_under_five_seconds() {
    let _serial = serial();
    let cache = tempfile::tempdir().unwrap();
    let t = Instant::now();
    locrin(cache.path()).arg("scan").assert().success();
    let ms = t.elapsed().as_millis();
    println!("cold scan: {ms} ms");
    assert!(ms < 5_000, "cold index took {ms} ms");
}

#[test]
#[ignore]
fn warm_single_file_check_under_300ms() {
    let _serial = serial();
    let cache = tempfile::tempdir().unwrap();
    locrin(cache.path()).arg("scan").assert().success();
    let file = "app/_layout.tsx";
    let t = Instant::now();
    let out = locrin(cache.path()).args(["check", file]).output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("warm single-file check: {ms} ms");
    // Exit 0 (clean) and 1 (findings) are both real work; exit 2 is an engine
    // error that would return in a few milliseconds and fake a fast benchmark.
    assert!(
        out.status.code() != Some(2),
        "check failed with exit 2, so the {ms} ms is not a real measurement: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ms < 300, "warm check took {ms} ms");
}

#[test]
#[ignore]
fn startup_under_50ms() {
    let _serial = serial();
    let t = Instant::now();
    let out = Command::cargo_bin("locrin").unwrap().arg("--help").output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("startup: {ms} ms");
    assert!(
        out.status.success(),
        "--help failed with {}, so the {ms} ms is not a real measurement: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ms < 50, "startup took {ms} ms");
}

/// Spec 3.4: a warm diff check on a typical pull request (30 files) under 1 s.
#[test]
#[ignore]
fn warm_thirty_file_check_under_one_second() {
    let _serial = serial();
    let cache = tempfile::tempdir().unwrap();
    locrin(cache.path()).arg("scan").assert().success();
    let dir = repo().join("app");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|x| x == "tsx" || x == "ts").unwrap_or(false))
        .map(|p| format!("app/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    files.sort();
    files.truncate(30);
    assert!(files.len() >= 10, "bench repo has too few files under app/ to stand in for a PR: {}", files.len());
    let t = Instant::now();
    let out = locrin(cache.path()).arg("check").args(&files).output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("warm {}-file check: {ms} ms", files.len());
    assert!(
        out.status.code() != Some(2),
        "check failed with exit 2, so the {ms} ms is not a real measurement: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ms < 1_000, "warm diff check took {ms} ms");
}
