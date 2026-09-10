//! The five tools the MCP server offers, and nothing about the protocol: that
//! is [`locrin_mcp`]'s, which calls in here through [`locrin_mcp::Handler`].
//!
//! Every tool answers with one compact JSON object as its text content, because
//! the caller is a model reading a transcript rather than a person reading a
//! terminal, and every argument it will not accept comes back as a
//! [`ToolError`] naming the argument that was wrong. An engine failure is a
//! `ToolError` too: a tool call must never take the server down, so the agent
//! reads what went wrong and moves on.

use std::path::{Path, PathBuf};

use locrin_core::baseline::{Baseline, BASELINE_FILE};
use locrin_core::config::{Config, CONFIG_FILE};
use locrin_core::index::{cache_path, Index, SCHEMA_VERSION};
use locrin_core::symbols::{self, Query};
use locrin_core::walk::canonical_root;
use locrin_mcp::{Handler, ToolError, ToolSpec};
use serde_json::{json, Value};

use crate::git::DiffScope;
use crate::run::{self, Options};

/// How many symbols a search answers with. An agent is deciding whether
/// something already exists, and a page of candidates it has to read through is
/// a worse answer than the ten best.
const SEARCH_LIMIT: usize = 10;

/// How much of a declaration's line is worth showing. Long enough to carry a
/// signature, short enough that ten of them stay a summary.
const SUMMARY_CHARS: usize = 120;

/// The author recorded for a finding an agent signed off. See
/// [`run::baseline_accept_as`] for why it is not the login name.
const AGENT_AUTHOR: &str = "agent via mcp";

pub struct Tools {
    root: PathBuf,
    offline: bool,
}

impl Tools {
    /// The root is canonicalised once here rather than at each call: the index
    /// keys its cache on the canonical root and reports paths relative to it, so
    /// a tool handed the root as typed would look in a different database than
    /// the one `locrin check` filled.
    pub fn new(root: PathBuf, offline: bool) -> Tools {
        Tools { root: canonical_root(&root), offline }
    }

    /// The whole repository, which is the scope every tool but `check_changes`
    /// works in.
    fn whole_repo(&self) -> Options {
        Options {
            root: self.root.clone(),
            paths: vec![],
            changed_only: false,
            json: false,
            offline: self.offline,
            diff: None,
        }
    }

    fn check_changes(&self, args: &Value) -> Result<String, ToolError> {
        let paths = opt_paths(args)?;
        let base = opt_str(args, "base")?;
        // Both scopes name their own files, so a call that gave both never said
        // which it meant. The command line rejects the same pair.
        if !paths.is_empty() && base.is_some() {
            return Err(ToolError("paths and base cannot be combined".into()));
        }
        let opts = Options {
            root: self.root.clone(),
            paths,
            changed_only: false,
            json: true,
            offline: self.offline,
            diff: base.map(DiffScope::Base),
        };
        let verdict = run::check(&opts).map_err(engine)?;
        Ok(locrin_reporters::agent::render(&verdict))
    }

    fn find_existing(&self, args: &Value) -> Result<String, ToolError> {
        let name = opt_str(args, "name")?;
        let intent = opt_str(args, "intent")?;
        let params = opt_u32(args, "params")?;
        let returns = opt_str(args, "returns")?;
        if name.is_none() && intent.is_none() {
            return Err(ToolError("find_existing needs at least one of intent or name".into()));
        }
        // `Index::open` creates the database it cannot find, so an unindexed
        // repository would silently answer "nothing exists" to every question
        // an agent asked before its first scan. That is the one answer this
        // tool must never give by accident.
        if !cache_path(&self.root).exists() {
            return Err(ToolError("no index: run locrin init or locrin scan first".into()));
        }
        let index = Index::open(&self.root).map_err(engine)?;
        let hits = symbols::search(&index, &Query { name, intent, params }, SEARCH_LIMIT).map_err(engine)?;
        let matches: Vec<Value> = hits
            .iter()
            .map(|m| {
                json!({
                    "file": m.rel,
                    "line": m.line,
                    "kind": m.kind,
                    "name": m.name,
                    "params": m.params,
                    "exported": m.exported,
                    "summary": line_text(&self.root, &m.rel, m.line),
                })
            })
            .collect();
        let mut out = json!({ "matches": matches });
        // `returns` is in the schema because the caller has the return type in
        // hand and would otherwise leave it out of the question entirely. It is
        // not indexed yet, and a silently dropped argument reads as a search
        // that considered it, so the result says so.
        if returns.is_some() {
            out["note"] = json!("return types are not indexed in version one");
        }
        Ok(out.to_string())
    }

    fn explain_finding(&self, args: &Value) -> Result<String, ToolError> {
        let id = req_str(args, "id")?;
        // The unfiltered set, not `check`'s: a finding already in the baseline
        // is exactly the one worth explaining, and the answer says so through
        // `accepted` rather than by pretending the finding is gone.
        let findings = run::full_findings(&self.root, &self.whole_repo(), true).map_err(engine)?;
        let Some(f) = findings.iter().find(|f| f.id == id) else {
            return Err(ToolError(format!("no current finding with id {id}")));
        };
        let mut out = serde_json::to_value(f).map_err(|e| ToolError(format!("{e:#}")))?;
        let description =
            locrin_rules::all_rules().iter().find(|r| r.id() == f.rule).map(|r| r.description()).unwrap_or_default();
        out["rule_description"] = json!(description);
        let baseline = Baseline::load(&self.root).map_err(engine)?;
        out["accepted"] = match baseline.entries.iter().find(|e| e.id == f.id) {
            Some(e) => json!({"reason": e.reason, "author": e.author, "date": e.date}),
            None => Value::Null,
        };
        Ok(out.to_string())
    }

    fn accept_finding(&self, args: &Value) -> Result<String, ToolError> {
        let id = req_str(args, "id")?;
        let reason = req_str(args, "reason")?;
        if reason.trim().is_empty() {
            return Err(ToolError("reason must not be empty".into()));
        }
        if !run::baseline_accept_as(&self.root, &id, &reason, AGENT_AUTHOR, self.offline).map_err(engine)? {
            return Err(ToolError(format!("no current finding with id {id}")));
        }
        let after = Baseline::load(&self.root).map_err(engine)?.entries.len();
        Ok(json!({"accepted": true, "id": id, "baseline_entries": after}).to_string())
    }

    fn status(&self) -> Result<String, ToolError> {
        let index_path = cache_path(&self.root);
        let exists = index_path.exists();
        // Opened only when it is already there: `status` answers what the
        // repository looks like now, and a question must not be what creates
        // the thing it asked about.
        let (files, last_verdict) = if exists {
            let index = Index::open(&self.root).map_err(engine)?;
            let files = index.all_files().map_err(engine)?.len();
            let note = index.meta_get("last_verdict").map_err(engine)?;
            (files, note.and_then(|n| serde_json::from_str(&n).ok()).unwrap_or(Value::Null))
        } else {
            (0, Value::Null)
        };

        let config_path = self.root.join(CONFIG_FILE);
        let present = config_path.exists();
        let config = Config::load(&self.root).map_err(engine)?;
        let baseline = Baseline::load(&self.root).map_err(engine)?;

        Ok(json!({
            "index": {
                "path": index_path.display().to_string(),
                "exists": exists,
                "files": files,
                "schema": SCHEMA_VERSION,
            },
            "config": {
                "path": config_path.display().to_string(),
                "present": present,
                "excludes": config.excludes.len(),
                "rule_overrides": config.rules.len(),
                "boundaries": config.boundaries.len(),
            },
            "baseline": {
                "path": self.root.join(BASELINE_FILE).display().to_string(),
                "entries": baseline.entries.len(),
            },
            "last_verdict": last_verdict,
        })
        .to_string())
    }
}

impl Handler for Tools {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "check_changes",
                description: "Run the quality gate and return the verdict: status, counts, and up to ten findings \
                              with their ids. Call it after writing code and before saying the work is done.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Files or directories to check, repo-relative or absolute. Omit for the whole repository. Cannot be combined with base.",
                        },
                        "base": {
                            "type": "string",
                            "description": "A git ref: check what differs from the merge base with it, plus untracked files. Cannot be combined with paths.",
                        },
                    },
                }),
            },
            ToolSpec {
                name: "find_existing",
                description: "Search the indexed symbols for something that already does this, before writing it \
                              again. Ranks on the parameter count first and the shared name tokens second.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "intent": {"type": "string", "description": "What the code would do, in prose."},
                        "name": {"type": "string", "description": "The name you were about to give it."},
                        "params": {"type": "integer", "description": "How many parameters it would take."},
                        "returns": {"type": "string", "description": "The return type. Accepted but not yet indexed."},
                    },
                    // Expressed in prose rather than as an `anyOf` of two
                    // `required` lists: the constraint is one of two, which
                    // every client renders badly, and the tool says the same
                    // thing in words when the call arrives empty.
                    "description": "At least one of intent or name is required.",
                }),
            },
            ToolSpec {
                name: "explain_finding",
                description: "Everything known about one finding by id: the rule and what it is for, the span, the \
                              evidence, the suggested fix, and whether it is already accepted in the baseline.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "The finding id from a check_changes verdict."},
                    },
                    "required": ["id"],
                }),
            },
            ToolSpec {
                name: "accept_finding",
                description: "Record one finding in the baseline as accepted debt, with the reason. Use it only when \
                              the finding is genuinely not worth fixing now; fixing the code is the other answer.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "The finding id from a check_changes verdict."},
                        "reason": {"type": "string", "description": "Why this is being accepted rather than fixed."},
                    },
                    "required": ["id", "reason"],
                }),
            },
            ToolSpec {
                name: "status",
                description: "What locrin knows about this repository: the index, the config, the baseline, and the \
                              last verdict recorded. Reads only; it never builds the index.",
                input_schema: json!({"type": "object", "properties": {}}),
            },
        ]
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<String, ToolError> {
        match name {
            "check_changes" => self.check_changes(args),
            "find_existing" => self.find_existing(args),
            "explain_finding" => self.explain_finding(args),
            "accept_finding" => self.accept_finding(args),
            "status" => self.status(),
            // The protocol layer rejects a name it does not know before it gets
            // here, so this arm is the two of them disagreeing.
            other => Err(ToolError(format!("unknown tool: {other}"))),
        }
    }
}

/// An engine failure as the agent sees it. `{e:#}` so the context chain comes
/// with it: "reading src/a.ts: permission denied" is actionable and
/// "permission denied" is not.
fn engine(e: anyhow::Error) -> ToolError {
    ToolError(format!("{e:#}"))
}

/// The declaration's own line, trimmed and capped, as the one line of context a
/// caller needs to tell two same-named symbols apart.
///
/// A file that cannot be read answers with nothing rather than failing the
/// search: the match itself is still true, and the summary is a convenience.
/// Capped in characters rather than bytes so the cap never splits a code point.
fn line_text(root: &Path, rel: &str, line: u32) -> String {
    let Ok(text) = std::fs::read_to_string(root.join(rel)) else { return String::new() };
    let Some(raw) = text.lines().nth(line.saturating_sub(1) as usize) else { return String::new() };
    raw.trim().chars().take(SUMMARY_CHARS).collect()
}

/// An optional string argument. A key that is present but is not a string is an
/// error rather than an absence: the caller meant something by it.
fn opt_str(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolError(format!("{key} must be a string"))),
    }
}

fn req_str(args: &Value, key: &str) -> Result<String, ToolError> {
    opt_str(args, key)?.ok_or_else(|| ToolError(format!("{key} is required")))
}

/// An optional count. A negative or fractional number is not a parameter count,
/// and saying so beats searching for a signature nothing can have.
fn opt_u32(args: &Value, key: &str) -> Result<Option<u32>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => match n.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Ok(Some(n)),
            None => Err(ToolError(format!("{key} must be a non-negative integer"))),
        },
        Some(_) => Err(ToolError(format!("{key} must be a non-negative integer"))),
    }
}

/// The optional path list. Absent is the whole repository, which is why an
/// empty list and a missing key mean the same thing here.
fn opt_paths(args: &Value) -> Result<Vec<PathBuf>, ToolError> {
    let list = match args.get("paths") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list,
        Some(_) => return Err(ToolError("paths must be an array of strings".into())),
    };
    list.iter()
        .map(|p| match p.as_str() {
            Some(s) => Ok(PathBuf::from(s)),
            None => Err(ToolError("paths must be an array of strings".into())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The argument readers are the part with no repository behind it, so they
    /// are checked here rather than through a server in `tests/mcp.rs`.
    #[test]
    fn argument_errors_name_the_argument() {
        let args = json!({"id": 7, "params": -1, "paths": [3]});
        assert_eq!(opt_str(&args, "id").unwrap_err().0, "id must be a string");
        assert_eq!(req_str(&json!({}), "id").unwrap_err().0, "id is required");
        assert_eq!(opt_u32(&args, "params").unwrap_err().0, "params must be a non-negative integer");
        assert_eq!(opt_paths(&args).unwrap_err().0, "paths must be an array of strings");
    }

    #[test]
    fn absent_and_null_arguments_read_the_same() {
        // `matches!` rather than `unwrap`: `ToolError` is not `Debug`, so the
        // failure has to be read off the pattern rather than printed.
        for args in [json!({}), json!({"name": null, "params": null, "paths": null})] {
            assert!(matches!(opt_str(&args, "name"), Ok(None)), "{args}");
            assert!(matches!(opt_u32(&args, "params"), Ok(None)), "{args}");
            assert!(matches!(opt_paths(&args), Ok(ref v) if v.is_empty()), "{args}");
        }
    }

    #[test]
    fn every_tool_declares_an_object_schema() {
        let tools = Tools::new(PathBuf::from("."), true);
        let names: Vec<&str> = tools.tools().iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["check_changes", "find_existing", "explain_finding", "accept_finding", "status"]);
        for t in tools.tools() {
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            assert!(t.input_schema["properties"].is_object(), "{}", t.name);
            assert!(!t.description.is_empty(), "{}", t.name);
        }
    }

    /// A long declaration is cut at a character boundary, and a line that is not
    /// there is not a failure.
    #[test]
    fn summaries_are_trimmed_and_capped() {
        let dir = tempfile::tempdir().unwrap();
        let long = format!("  export function wide({}) {{}}", "é".repeat(200));
        std::fs::write(dir.path().join("a.ts"), format!("{long}\nsecond\n")).unwrap();
        let summary = line_text(dir.path(), "a.ts", 1);
        assert_eq!(summary.chars().count(), SUMMARY_CHARS);
        assert!(summary.starts_with("export function wide("), "{summary}");
        assert_eq!(line_text(dir.path(), "a.ts", 2), "second");
        assert_eq!(line_text(dir.path(), "a.ts", 99), "");
        assert_eq!(line_text(dir.path(), "missing.ts", 1), "");
    }
}
