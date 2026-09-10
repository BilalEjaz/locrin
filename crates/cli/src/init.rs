use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context};
use locrin_core::baseline::BASELINE_FILE;
use locrin_core::config::{Config, CONFIG_FILE};
use locrin_core::walk::{source_files, WalkOptions};
use serde_json::{json, Map, Value};

use crate::run;

/// What `init` did to one file. `Skipped` carries the one-line instruction the
/// operator needs to finish the job by hand.
pub enum Touch {
    Wrote,
    Updated,
    Unchanged,
    Skipped(String),
}

/// Everything one `init` run has to say, so the caller decides how to print it.
pub struct Report {
    pub touched: Vec<(String, Touch)>,
    pub files_indexed: usize,
    pub baseline_entries: Option<usize>,
}

/// Written only when `locrin.toml` is absent. Every setting is commented out, so
/// the file the operator finds loads as `Config::default()` and reads as the
/// documentation of what they may turn on.
pub const CONFIG_TEMPLATE: &str = r#"# Locrin configuration. Every key is optional; an absent key keeps the default.
# Docs: README.md, section "Config".

# Globs the engine never reads (generated code, vendored code).
# excludes = ["src/generated/**"]

# Files where debug output is allowed (scripts, config files, bin).
# debug_allowed = ["**/scripts/**", "**/*.config.*", "**/bin/**"]

# Extra entry points for the dead-code rules, beyond package.json main/bin/exports.
# entry_points = ["src/worker.ts"]

# Import directions. Each entry sets exactly one of forbid or allow.
# [[boundaries]]
# name = "ui never imports the database"
# from = "src/ui/**"
# forbid = ["src/db/**"]

# [framework]
# auth_middleware = ["requireAuth"]
# server_paths = ["server/**"]

# Per-rule overrides. secret-exposed is locked and ignores these.
# [rules.dead-file]
# enabled = true
# [rules.unused-import]
# severity = "medium"
"#;

pub const PRE_COMMIT_SCRIPT: &str =
    "#!/bin/sh\n# Installed by locrin init. Remove this file to uninstall.\nexec locrin hook pre-commit\n";

const SETTINGS_FILE: &str = ".claude/settings.json";
const MCP_FILE: &str = ".mcp.json";
const PRE_COMMIT_FILE: &str = ".git/hooks/pre-commit";

/// The commands the settings entries name. Matching is by prefix, so an operator
/// who added a flag keeps the hook they wrote.
const POST_EDIT_COMMAND: &str = "locrin hook post-edit";
const STOP_COMMAND: &str = "locrin hook stop";

/// Wires a repository up: config, agent hooks, MCP server, git hook, the first
/// scan and the baseline. Nothing here overwrites a file locrin did not write,
/// and a second run reports `Unchanged` for every step.
///
/// `progress` receives the lines a person watching the first scan needs; the
/// caller decides where they go, which is not stdout: they name no file the
/// command touched.
pub fn run(root: &Path, offline: bool, progress: &mut dyn FnMut(&str)) -> anyhow::Result<Report> {
    let mut touched = Vec::new();

    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() {
        touched.push((CONFIG_FILE.to_string(), Touch::Unchanged));
    } else {
        std::fs::write(&config_path, CONFIG_TEMPLATE).with_context(|| format!("writing {}", config_path.display()))?;
        touched.push((CONFIG_FILE.to_string(), Touch::Wrote));
    }

    touched.push(merge_file(root, SETTINGS_FILE, merge_settings)?);
    touched.push(merge_file(root, MCP_FILE, merge_mcp)?);
    touched.push(install_pre_commit(root)?);

    // The count is walked here rather than taken from the scan because the scan
    // reports it only once it is over: the first scan on a cold tree is the OS
    // reading the repository, and a person watching a blank line for half a
    // minute assumes a hang. A second walk costs milliseconds against that.
    let excludes = Config::load(root)?.excludes;
    let n = source_files(root, &WalkOptions { excludes })?.len();
    progress(&format!("indexing {n} source file(s) under {}", root.display()));
    let started = Instant::now();
    let (files, _changed) = run::scan(root, offline)?;
    let elapsed = started.elapsed().as_secs_f64();
    let mut report = Report { touched, files_indexed: files, baseline_entries: None };
    progress(&format!("indexed {} file(s) in {elapsed:.1} s", report.files_indexed));

    // The scan above has warmed the findings cache, so this second pass over the
    // repository is served from it.
    if root.join(BASELINE_FILE).exists() {
        report.touched.push((BASELINE_FILE.to_string(), Touch::Unchanged));
    } else {
        let entries = run::baseline_create(root, offline)?;
        report.touched.push((BASELINE_FILE.to_string(), Touch::Wrote));
        report.baseline_entries = Some(entries);
    }
    Ok(report)
}

/// Reads `rel`, runs it through `merge`, and writes the result back only when
/// the merge changed something. Not writing is what makes a second run leave the
/// file's bytes alone even where the operator has reformatted it.
fn merge_file(
    root: &Path,
    rel: &str,
    merge: fn(Option<&str>) -> anyhow::Result<(String, bool)>,
) -> anyhow::Result<(String, Touch)> {
    let path = root.join(rel);
    let existing = match path.exists() {
        true => Some(std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?),
        false => None,
    };
    let (text, changed) = merge(existing.as_deref())?;
    let touch = match (existing.is_some(), changed) {
        (true, false) => Touch::Unchanged,
        (had_file, _) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
            if had_file {
                Touch::Updated
            } else {
                Touch::Wrote
            }
        }
    };
    Ok((rel.to_string(), touch))
}

/// Installs the git pre-commit hook, or says who owns it instead.
///
/// A hook whose text is exactly the one locrin writes is locrin's own, so a
/// second run reports it unchanged rather than skipping the file it installed a
/// moment ago.
fn install_pre_commit(root: &Path) -> anyhow::Result<(String, Touch)> {
    let rel = PRE_COMMIT_FILE.to_string();
    if root.join(".husky").is_dir() {
        let why = "husky detected: add `locrin hook pre-commit` to .husky/pre-commit";
        return Ok((rel, Touch::Skipped(why.to_string())));
    }
    if !root.join(".git/hooks").is_dir() {
        let why = "not a git repository: no pre-commit hook installed";
        return Ok((rel, Touch::Skipped(why.to_string())));
    }
    let path = root.join(PRE_COMMIT_FILE);
    if path.exists() {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current == PRE_COMMIT_SCRIPT {
            return Ok((rel, Touch::Unchanged));
        }
        let why = "a pre-commit hook already exists: add `locrin hook pre-commit` to it";
        return Ok((rel, Touch::Skipped(why.to_string())));
    }
    std::fs::write(&path, PRE_COMMIT_SCRIPT).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("marking {} executable", path.display()))?;
    }
    Ok((rel, Touch::Wrote))
}

/// Merges locrin's two Claude Code hooks into `.claude/settings.json`.
///
/// Returns the text to write and whether the parsed value changed. Comparing
/// values rather than text is what lets an operator reformat the file without a
/// re-run reporting an update.
pub fn merge_settings(existing: Option<&str>) -> anyhow::Result<(String, bool)> {
    let before = parse_object(existing, SETTINGS_FILE)?;
    let mut after = before.clone();
    let hooks = child_object(&mut after, "hooks", SETTINGS_FILE)?;
    ensure_hook(hooks, "PostToolUse", Some("Edit|Write|MultiEdit"), POST_EDIT_COMMAND, 5, SETTINGS_FILE)?;
    ensure_hook(hooks, "Stop", None, STOP_COMMAND, 10, SETTINGS_FILE)?;
    Ok((render(&after), after != before))
}

/// Merges the locrin MCP server into `.mcp.json`. An entry that is already there
/// is left exactly as it is: init ensures a locrin server exists, it does not
/// enforce how the operator launches it.
pub fn merge_mcp(existing: Option<&str>) -> anyhow::Result<(String, bool)> {
    let before = parse_object(existing, MCP_FILE)?;
    let mut after = before.clone();
    let servers = child_object(&mut after, "mcpServers", MCP_FILE)?;
    if !servers.contains_key("locrin") {
        servers.insert("locrin".to_string(), json!({"command": "locrin", "args": ["mcp"]}));
    }
    Ok((render(&after), after != before))
}

/// An absent file is `{}`. A file that is not a JSON object is an error naming
/// the path: init reports it rather than replacing whatever the operator has.
fn parse_object(existing: Option<&str>, rel: &str) -> anyhow::Result<Map<String, Value>> {
    let Some(text) = existing else { return Ok(Map::new()) };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(text).with_context(|| format!("invalid {rel}"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => bail!("{rel} is not a JSON object; locrin init will not overwrite it"),
    }
}

/// The object under `key`, created empty when absent. A key holding something
/// else is an error for the same reason a non-object file is.
fn child_object<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
    rel: &str,
) -> anyhow::Result<&'a mut Map<String, Value>> {
    let entry = parent.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
    match entry.as_object_mut() {
        Some(map) => Ok(map),
        None => bail!("{rel}: \"{key}\" is not a JSON object; locrin init will not overwrite it"),
    }
}

/// Ensures one hook event holds locrin's command.
///
/// "Holds" is decided by scanning every command in the whole event array for one
/// that starts with `command`, not by looking for an entry shaped the way init
/// writes it: an operator who moved the command into their own entry, or added a
/// flag to it, keeps their arrangement.
fn ensure_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    timeout: u64,
    rel: &str,
) -> anyhow::Result<()> {
    let entry = hooks.entry(event.to_string()).or_insert_with(|| Value::Array(vec![]));
    let Some(entries) = entry.as_array_mut() else {
        bail!("{rel}: hooks.{event} is not an array; locrin init will not overwrite it");
    };
    let present = entries
        .iter()
        .filter_map(|e| e.get("hooks")?.as_array())
        .flatten()
        .filter_map(|h| h.get("command")?.as_str())
        .any(|c| c.starts_with(command));
    if present {
        return Ok(());
    }
    let mut new_entry = Map::new();
    if let Some(matcher) = matcher {
        new_entry.insert("matcher".to_string(), Value::String(matcher.to_string()));
    }
    new_entry.insert("hooks".to_string(), json!([{"type": "command", "command": command, "timeout": timeout}]));
    entries.push(Value::Object(new_entry));
    Ok(())
}

/// Pretty JSON with the trailing newline every other file in a repository has.
fn render(value: &Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged_settings(existing: Option<&str>) -> Value {
        let (text, _) = merge_settings(existing).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn merged_mcp(existing: Option<&str>) -> Value {
        let (text, _) = merge_mcp(existing).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn merge_settings_creates_both_hooks_from_nothing() {
        let v = merged_settings(None);
        assert_eq!(
            v["hooks"],
            json!({
                "PostToolUse": [{
                    "matcher": "Edit|Write|MultiEdit",
                    "hooks": [{"type": "command", "command": "locrin hook post-edit", "timeout": 5}]
                }],
                "Stop": [{
                    "hooks": [{"type": "command", "command": "locrin hook stop", "timeout": 10}]
                }]
            }),
            "{v}"
        );
    }

    #[test]
    fn merge_settings_keeps_existing_hooks_and_keys() {
        let existing = json!({
            "permissions": {"allow": ["Bash(ls:*)"]},
            "hooks": {
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "audit"}]}],
                "PostToolUse": [{"matcher": "Write", "hooks": [{"type": "command", "command": "prettier"}]}]
            }
        })
        .to_string();
        let v = merged_settings(Some(&existing));
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)", "{v}");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "audit", "{v}");
        let post = v["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 2, "{v}");
        assert_eq!(post[0]["hooks"][0]["command"], "prettier", "{v}");
        assert_eq!(post[1]["matcher"], "Edit|Write|MultiEdit", "{v}");
        assert_eq!(post[1]["hooks"][0]["command"], "locrin hook post-edit", "{v}");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "locrin hook stop", "{v}");
    }

    #[test]
    fn merge_settings_is_idempotent() {
        let (once, changed) = merge_settings(None).unwrap();
        assert!(changed);
        let (twice, changed) = merge_settings(Some(&once)).unwrap();
        assert!(!changed, "a second merge reported a change");
        assert_eq!(once, twice);
    }

    /// The rule is a prefix, not equality: an operator who added a flag to the
    /// command still has the hook, and init must not add a second one beside it.
    #[test]
    fn an_existing_locrin_command_is_not_duplicated() {
        let existing = json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Write",
                    "hooks": [{"type": "command", "command": "locrin hook post-edit --root ."}]
                }]
            }
        })
        .to_string();
        let (_, changed) = merge_settings(Some(&existing)).unwrap();
        assert!(changed, "the Stop hook was still missing");
        let v = merged_settings(Some(&existing));
        let post = v["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 1, "{v}");
        assert_eq!(post[0]["hooks"][0]["command"], "locrin hook post-edit --root .", "{v}");
    }

    /// Pinned so the prefix rule is a decision rather than an accident: a command
    /// that only mentions locrin somewhere inside it is not the hook, so the hook
    /// is added beside it.
    #[test]
    fn a_wrapped_command_is_not_recognised() {
        let existing = json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Write",
                    "hooks": [{"type": "command", "command": "sh -c \"locrin hook post-edit || true\""}]
                }]
            }
        })
        .to_string();
        let v = merged_settings(Some(&existing));
        let post = v["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 2, "{v}");
        assert_eq!(post[1]["hooks"][0]["command"], "locrin hook post-edit", "{v}");
    }

    #[test]
    fn merge_settings_refuses_a_non_object() {
        let e = merge_settings(Some("[]")).unwrap_err().to_string();
        assert!(e.contains(".claude/settings.json"), "{e}");
    }

    #[test]
    fn merge_mcp_creates_the_server_from_nothing() {
        let v = merged_mcp(None);
        assert_eq!(v["mcpServers"], json!({"locrin": {"command": "locrin", "args": ["mcp"]}}), "{v}");
    }

    #[test]
    fn merge_mcp_keeps_another_server() {
        let existing = json!({"mcpServers": {"other": {"command": "other-server"}}}).to_string();
        let v = merged_mcp(Some(&existing));
        assert_eq!(v["mcpServers"]["other"]["command"], "other-server", "{v}");
        assert_eq!(v["mcpServers"]["locrin"]["args"][0], "mcp", "{v}");
    }

    #[test]
    fn merge_mcp_is_idempotent() {
        let (once, changed) = merge_mcp(None).unwrap();
        assert!(changed);
        let (twice, changed) = merge_mcp(Some(&once)).unwrap();
        assert!(!changed, "a second merge reported a change");
        assert_eq!(once, twice);
    }

    /// An operator who points the entry at their own wrapper keeps it: init
    /// ensures a locrin server exists, it does not enforce how it is launched.
    #[test]
    fn merge_mcp_leaves_an_existing_locrin_entry() {
        let existing =
            json!({"mcpServers": {"locrin": {"command": "cargo", "args": ["run", "--", "mcp"]}}}).to_string();
        let (_, changed) = merge_mcp(Some(&existing)).unwrap();
        assert!(!changed, "an existing locrin entry was rewritten");
        let v = merged_mcp(Some(&existing));
        assert_eq!(v["mcpServers"]["locrin"]["command"], "cargo", "{v}");
    }

    #[test]
    fn merge_mcp_refuses_a_non_object() {
        let e = merge_mcp(Some("[]")).unwrap_err().to_string();
        assert!(e.contains(".mcp.json"), "{e}");
    }
}
