mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use common::{copy_fixture, locrin, walkdir};
use serde_json::{json, Value};

/// The index database a run wrote, wherever the cache key put it. The path is
/// `<LOCRIN_CACHE_DIR>/<key>/index.db` and the key is a hash of the canonical
/// root, so a test that wants the file finds it rather than deriving it.
fn index_db(dir: &Path) -> PathBuf {
    walkdir(&dir.join(".cache"))
        .into_iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("index.db"))
        .expect("the run should have written an index database")
}

/// A running `locrin mcp` with its pipes.
///
/// The child is held alongside them because the server only returns when its
/// stdin closes: a test that dropped the handles in the wrong order, or never
/// dropped them, would leave a process blocked on a pipe nobody writes to.
struct Session {
    child: Child,
    out: BufReader<ChildStdout>,
    /// An `Option` so [`finish`] can close it while the child is still owned.
    inp: Option<ChildStdin>,
}

/// Starts the server the way Claude Code does: no arguments beyond the
/// subcommand, the project directory as the working directory, stdio for the
/// protocol. Stderr is inherited rather than piped, so a warning the server
/// prints reaches the test output instead of filling a pipe nobody drains.
fn session(dir: &Path) -> Session {
    let mut cmd = locrin(dir);
    let mut child = cmd
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the server starts");
    let out = BufReader::new(child.stdout.take().expect("piped stdout"));
    let inp = child.stdin.take().expect("piped stdin");
    Session { child, out, inp: Some(inp) }
}

/// One request out, one response line back.
fn rpc(io: &mut Session, id: i64, method: &str, params: Value) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    {
        let stdin = io.inp.as_mut().expect("the session is open");
        writeln!(stdin, "{request}").expect("the server is reading");
        stdin.flush().expect("the server is reading");
    }
    let mut line = String::new();
    io.out.read_line(&mut line).expect("a response line");
    assert!(!line.trim().is_empty(), "the server closed stdout on {method}");
    serde_json::from_str(&line).unwrap_or_else(|e| panic!("the response to {method} is not JSON: {e}: {line}"))
}

/// The handshake every client sends before it calls anything.
fn handshake(io: &mut Session) {
    let r = rpc(io, 1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}}));
    assert_eq!(r["result"]["serverInfo"]["name"], "locrin", "{r}");
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let stdin = io.inp.as_mut().expect("the session is open");
    writeln!(stdin, "{note}").expect("the server is reading");
    stdin.flush().expect("the server is reading");
}

/// One `tools/call`, as the result object. A tool that failed is a result with
/// `isError`, so this never unwraps a protocol error away.
fn call(io: &mut Session, id: i64, name: &str, args: Value) -> Value {
    let r = rpc(io, id, "tools/call", json!({"name": name, "arguments": args}));
    assert!(r.get("error").is_none(), "{name} came back as a protocol error: {r}");
    r["result"].clone()
}

/// The text content of a tool result.
fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text content: {result}")).to_string()
}

/// The text content of a tool result, parsed. Every tool answers with one JSON
/// object, so a result that does not parse is the failure.
fn parsed(result: &Value) -> Value {
    let raw = text(result);
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("the tool result is not JSON: {e}: {raw}"))
}

/// Closes stdin so `serve` returns at EOF, then waits for the child.
fn finish(mut io: Session) {
    drop(io.inp.take());
    let status = io.child.wait().expect("the server exits");
    assert!(status.success(), "the server exited with {status}");
}

#[test]
fn initialize_then_list_names_the_five_tools() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let r = rpc(&mut io, 2, "tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().expect("a tools array").clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().expect("a name")).collect();
    assert_eq!(names, vec!["check_changes", "find_existing", "explain_finding", "accept_finding", "status"]);
    for t in &tools {
        assert!(t["description"].as_str().is_some_and(|d| !d.is_empty()), "{t}");
        assert!(t["inputSchema"].is_object(), "{t}");
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
    }
    finish(io);
}

#[test]
fn check_changes_returns_the_agent_verdict() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let result = call(&mut io, 2, "check_changes", json!({}));
    assert!(result.get("isError").is_none(), "{result}");
    let v = parsed(&result);
    assert_eq!(v["status"], "block", "{v}");
    let findings = v["findings"].as_array().expect("findings");
    assert!(!findings.is_empty(), "{v}");
    assert!(findings.len() <= 10, "{v}");
    assert!(findings[0].get("source").is_none(), "the agent view never carries file contents: {v}");
    finish(io);
}

#[test]
fn check_changes_with_paths_narrows() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let result = call(&mut io, 2, "check_changes", json!({"paths": ["src/clean.ts"]}));
    assert!(result.get("isError").is_none(), "{result}");
    let v = parsed(&result);
    assert_eq!(v["status"], "pass", "{v}");
    finish(io);
}

#[test]
fn check_changes_rejects_paths_with_base() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let result = call(&mut io, 2, "check_changes", json!({"paths": ["src/clean.ts"], "base": "main"}));
    assert_eq!(result["isError"], true, "{result}");
    let message = text(&result);
    assert!(message.contains("paths"), "{message}");
    assert!(message.contains("base"), "{message}");
    finish(io);
}

#[test]
fn find_existing_ranks_by_name_tokens() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    // The index is a by-product of a check, so the search has something to read.
    call(&mut io, 2, "check_changes", json!({}));

    let result = call(&mut io, 3, "find_existing", json!({"name": "ok"}));
    assert!(result.get("isError").is_none(), "{result}");
    let v = parsed(&result);
    let matches = v["matches"].as_array().expect("matches").clone();
    let hit = matches
        .iter()
        .find(|m| m["name"] == "ok")
        .unwrap_or_else(|| panic!("the exported function was not found: {v}"));
    assert_eq!(hit["file"], "src/clean.ts", "{hit}");
    assert_eq!(hit["kind"], "function", "{hit}");
    assert_eq!(hit["line"], 1, "{hit}");
    assert_eq!(hit["exported"], true, "{hit}");
    assert!(hit["summary"].as_str().expect("a summary").contains("function ok"), "{hit}");
    finish(io);
}

#[test]
fn find_existing_without_an_index_says_so() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let result = call(&mut io, 2, "find_existing", json!({"name": "ok"}));
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(text(&result), "no index: run locrin init or locrin scan first");
    finish(io);
}

#[test]
fn find_existing_on_an_empty_index_says_so() {
    let dir = copy_fixture();
    // A scan fills the index, and then its schema stamp is walked back to a
    // version this build does not know: exactly what an upgraded binary meets
    // on an index its predecessor wrote. `Index::open` rebuilds that into an
    // empty database, so the file is there and has nothing in it, and the
    // first question an agent asks after the upgrade must not be answered
    // "nothing exists".
    let status = locrin(dir.path()).arg("scan").status().expect("the scan runs");
    assert!(status.success(), "the scan exited with {status}");
    rusqlite::Connection::open(index_db(dir.path()))
        .expect("the index database opens")
        .execute("UPDATE meta SET value = '4' WHERE key = 'schema_version'", [])
        .expect("the stamp is writable");

    let mut io = session(dir.path());
    handshake(&mut io);
    let result = call(&mut io, 2, "find_existing", json!({"name": "ok"}));
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(text(&result), "index is empty: run locrin scan first");
    finish(io);
}

#[test]
fn explain_finding_describes_a_finding() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let verdict = parsed(&call(&mut io, 2, "check_changes", json!({})));
    let id = verdict["findings"][0]["id"].as_str().expect("an id").to_string();

    let result = call(&mut io, 3, "explain_finding", json!({"id": id}));
    assert!(result.get("isError").is_none(), "{result}");
    let v = parsed(&result);
    assert_eq!(v["id"], json!(id), "{v}");
    assert_eq!(v["file"], "src/dirty.ts", "{v}");
    assert!(v["rule"].as_str().is_some_and(|r| !r.is_empty()), "{v}");
    assert!(v["rule_description"].as_str().is_some_and(|d| !d.is_empty()), "{v}");
    assert!(v["span"]["start_line"].is_number(), "{v}");
    assert!(v["evidence"].is_string(), "{v}");
    assert!(v["fix"].is_string(), "{v}");
    assert_eq!(v["accepted"], Value::Null, "nothing has been accepted yet: {v}");

    let missing = call(&mut io, 4, "explain_finding", json!({"id": "0000000000000000"}));
    assert_eq!(missing["isError"], true, "{missing}");
    assert_eq!(text(&missing), "no current finding with id 0000000000000000");
    finish(io);
}

#[test]
fn accept_finding_then_explain_shows_the_acceptance() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let verdict = parsed(&call(&mut io, 2, "check_changes", json!({})));
    let id = verdict["findings"][0]["id"].as_str().expect("an id").to_string();

    let accepted = call(&mut io, 3, "accept_finding", json!({"id": id, "reason": "tracked in TICKET-1"}));
    assert!(accepted.get("isError").is_none(), "{accepted}");
    let a = parsed(&accepted);
    assert_eq!(a["accepted"], true, "{a}");
    assert_eq!(a["id"], json!(id), "{a}");
    assert_eq!(a["baseline_entries"], 1, "{a}");

    let v = parsed(&call(&mut io, 4, "explain_finding", json!({"id": id})));
    assert_eq!(v["accepted"]["author"], "agent via mcp", "{v}");
    assert_eq!(v["accepted"]["reason"], "tracked in TICKET-1", "{v}");
    assert!(v["accepted"]["date"].as_str().is_some_and(|d| d.len() == 10), "{v}");

    let missing = call(&mut io, 5, "accept_finding", json!({"id": "0000000000000000", "reason": "why"}));
    assert_eq!(missing["isError"], true, "{missing}");
    assert_eq!(text(&missing), "no current finding with id 0000000000000000");

    let empty = call(&mut io, 6, "accept_finding", json!({"id": id, "reason": ""}));
    assert_eq!(empty["isError"], true, "{empty}");
    assert!(text(&empty).contains("reason"), "{empty}");
    finish(io);
}

#[test]
fn status_reports_the_index_config_and_last_verdict() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    call(&mut io, 2, "check_changes", json!({}));

    let v = parsed(&call(&mut io, 3, "status", json!({})));
    assert_eq!(v["index"]["exists"], true, "{v}");
    assert!(v["index"]["files"].as_u64().expect("a file count") >= 2, "{v}");
    assert!(v["index"]["schema"].is_string(), "{v}");
    assert!(v["index"]["path"].as_str().is_some_and(|p| !p.is_empty()), "{v}");
    assert_eq!(v["config"]["present"], false, "the fixture has no locrin.toml: {v}");
    assert_eq!(v["config"]["excludes"], 0, "{v}");
    assert_eq!(v["baseline"]["entries"], 0, "{v}");
    assert_eq!(v["last_verdict"]["status"], "block", "{v}");
    finish(io);
}

#[test]
fn status_never_creates_an_index() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    let v = parsed(&call(&mut io, 2, "status", json!({})));
    assert_eq!(v["index"]["exists"], false, "{v}");
    assert_eq!(v["index"]["files"], 0, "{v}");
    let path = v["index"]["path"].as_str().expect("a path").to_string();
    assert!(!Path::new(&path).exists(), "status created the index at {path}");
    finish(io);
    assert!(!Path::new(&path).exists(), "the index appeared at {path}");
}

#[test]
fn stdout_carries_nothing_but_messages() {
    let dir = copy_fixture();
    let mut io = session(dir.path());
    handshake(&mut io);
    // A spread of the calls a client actually makes, including one that fails
    // and one that runs the engine, which is where a stray print would appear.
    rpc(&mut io, 2, "tools/list", json!({}));
    call(&mut io, 3, "check_changes", json!({}));
    call(&mut io, 4, "status", json!({}));
    call(&mut io, 5, "find_existing", json!({}));
    rpc(&mut io, 6, "ping", json!({}));

    // Nothing is owed after the last response, so the rest of stdout is EOF.
    drop(io.inp.take());
    let mut rest = String::new();
    io.out.read_to_string(&mut rest).expect("stdout reads to EOF");
    assert_eq!(rest, "", "the server wrote past its last response: {rest}");
    let status = io.child.wait().expect("the server exits");
    assert!(status.success(), "the server exited with {status}");
}
