use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::finding::Severity;

pub const CONFIG_FILE: &str = "locrin.toml";

/// A per-rule override read from `[rules.<id>]`. Both fields are optional so an
/// absent key keeps the rule's own default rather than resetting it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RuleOverride {
    pub enabled: Option<bool>,
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub excludes: Vec<String>,
    pub debug_allowed: Vec<String>,
    pub rules: BTreeMap<String, RuleOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            excludes: vec![],
            debug_allowed: vec!["**/scripts/**".into(), "**/*.config.*".into(), "**/bin/**".into()],
            rules: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Reads `locrin.toml` from the repository root. An absent file is not an
    /// error: it means the defaults. A present but unparsable file is an error
    /// naming the path, so the operator can find the file to fix.
    pub fn load(repo_root: &Path) -> anyhow::Result<Config> {
        let path = repo_root.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
    }

    /// Rules are on unless the config turns them off.
    pub fn rule_enabled(&self, id: &str) -> bool {
        self.rules.get(id).and_then(|r| r.enabled).unwrap_or(true)
    }

    /// The configured severity for a rule, or the rule's own default.
    pub fn severity_for(&self, id: &str, default: Severity) -> Severity {
        self.rules.get(id).and_then(|r| r.severity).unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Severity;
    use std::path::{Path, PathBuf};

    /// Removes the temp directory on the way out of the test, including when an
    /// assertion panics part way through.
    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// A fresh directory: a leftover from an earlier failed run would otherwise
    /// leave a stale config file behind and decide the test for us.
    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("gate-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn path(c: &Cleanup) -> &Path {
        c.0.as_path()
    }

    #[test]
    fn defaults_when_absent() {
        let dir = fresh("config");
        let c = Config::load(path(&dir)).unwrap();
        assert!(c.excludes.is_empty());
        assert!(c.debug_allowed.iter().any(|g| g.contains("scripts")));
        assert!(c.rule_enabled("leftover-debug"));
        assert_eq!(c.severity_for("leftover-debug", Severity::High), Severity::High);
    }

    #[test]
    fn parses_overrides() {
        let dir = fresh("config2");
        std::fs::write(
            path(&dir).join(CONFIG_FILE),
            "excludes = [\"src/gen/**\"]\n[rules.leftover-debug]\nseverity = \"low\"\n[rules.leftover-agent-marker]\nenabled = false\n",
        )
        .unwrap();
        let c = Config::load(path(&dir)).unwrap();
        assert_eq!(c.excludes, vec!["src/gen/**"]);
        assert_eq!(c.severity_for("leftover-debug", Severity::High), Severity::Low);
        assert!(!c.rule_enabled("leftover-agent-marker"));
        assert!(c.rule_enabled("leftover-commented-code"));
    }

    #[test]
    fn invalid_file_is_an_error_naming_the_path() {
        let dir = fresh("config3");
        std::fs::write(path(&dir).join(CONFIG_FILE), "excludes = [").unwrap();
        let err = Config::load(path(&dir)).unwrap_err().to_string();
        assert!(err.contains(CONFIG_FILE));
    }
}
