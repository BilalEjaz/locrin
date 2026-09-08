//! The two project files resolution reads: `tsconfig.json` (aliases) and
//! `package.json` (entry points, workspaces). Both are read leniently: a file
//! that is missing or unparsable simply contributes nothing.

use std::path::Path;

use serde_json::Value;

/// Removes `//` and `/* */` comments and trailing commas outside strings, so a
/// tsconfig.json (which allows both) parses as JSON.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

pub fn parent_dir(rel: &str) -> String {
    match rel.rfind('/') {
        Some(i) => rel[..i].to_string(),
        None => String::new(),
    }
}

pub fn join(dir: &str, tail: &str) -> String {
    if dir.is_empty() {
        tail.to_string()
    } else {
        format!("{dir}/{tail}")
    }
}

/// Collapses `.` and `..` segments and a leading `./`. None when `..` would climb
/// above the repository root, which no import from inside it may do.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn with_json_ext(p: &str) -> String {
    if p.ends_with(".json") {
        p.to_string()
    } else {
        format!("{p}.json")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TsConfig {
    /// `compilerOptions.baseUrl`, repo-relative, from whichever file in the extends chain set it.
    pub base_url: Option<String>,
    /// Directory of the file that declared `paths`, repo-relative.
    pub paths_dir: String,
    /// `compilerOptions.paths` in declaration order: pattern with at most one `*`, and its targets.
    pub paths: Vec<(String, Vec<String>)>,
}

impl TsConfig {
    pub fn load(root: &Path) -> TsConfig {
        Self::load_file(root, "tsconfig.json", 0).unwrap_or_default()
    }

    /// The directory `paths` targets are relative to.
    pub fn paths_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or(&self.paths_dir)
    }

    fn load_file(root: &Path, rel: &str, depth: usize) -> Option<TsConfig> {
        if depth > 5 {
            return None;
        }
        let text = std::fs::read_to_string(root.join(rel)).ok()?;
        let value: Value = serde_json::from_str(&strip_jsonc(&text)).ok()?;
        let dir = parent_dir(rel);
        let mut cfg = value
            .get("extends")
            .and_then(Value::as_str)
            .map(|e| {
                if e.starts_with('.') {
                    normalize(&join(&dir, e)).unwrap_or_default()
                } else {
                    format!("node_modules/{e}")
                }
            })
            .and_then(|p| Self::load_file(root, &with_json_ext(&p), depth + 1))
            .unwrap_or_default();
        let opts = value.get("compilerOptions");
        if let Some(base) = opts.and_then(|o| o.get("baseUrl")).and_then(Value::as_str) {
            cfg.base_url = Some(normalize(&join(&dir, base)).unwrap_or_default());
        }
        if let Some(paths) = opts.and_then(|o| o.get("paths")).and_then(Value::as_object) {
            cfg.paths_dir = dir;
            cfg.paths = paths
                .iter()
                .map(|(k, v)| {
                    let targets = v
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
                        .unwrap_or_default();
                    (k.clone(), targets)
                })
                .collect();
        }
        Some(cfg)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageJson {
    pub name: Option<String>,
    pub main: Option<String>,
    pub bin: Vec<String>,
    /// Every string leaf under `exports`, whatever the nesting of conditions and subpaths.
    pub exports: Vec<String>,
    pub workspaces: Vec<String>,
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(o) => o.values().for_each(|x| collect_strings(x, out)),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

impl PackageJson {
    pub fn load(dir: &Path) -> Option<PackageJson> {
        let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut p = PackageJson {
            name: v.get("name").and_then(Value::as_str).map(String::from),
            main: v.get("main").and_then(Value::as_str).map(String::from),
            ..PackageJson::default()
        };
        match v.get("bin") {
            Some(Value::String(s)) => p.bin.push(s.clone()),
            Some(Value::Object(o)) => p.bin.extend(o.values().filter_map(Value::as_str).map(String::from)),
            _ => {}
        }
        if let Some(e) = v.get("exports") {
            collect_strings(e, &mut p.exports);
        }
        p.workspaces = match v.get("workspaces") {
            Some(Value::Object(o)) => string_list(o.get("packages")),
            other => string_list(other),
        };
        Some(p)
    }

    /// Every file this package points at, as repo-relative paths under `dir_rel`.
    /// Only `exports` leaves that start with `./` are files; the rest are conditions.
    pub fn entry_files(&self, dir_rel: &str) -> Vec<String> {
        self.main
            .iter()
            .chain(self.bin.iter())
            .chain(self.exports.iter().filter(|e| e.starts_with("./")))
            .filter_map(|p| normalize(&join(dir_rel, p)))
            .collect()
    }
}

/// Workspace packages as (name, repo-relative dir). Globs of the form `dir/*` are
/// expanded one level and plain directories are taken as they are; anything
/// fancier is ignored, which covers `packages/*` and `apps/*` in practice.
pub fn workspace_packages(root: &Path, workspaces: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pattern in workspaces {
        let pattern = pattern.trim_end_matches('/');
        let dirs: Vec<String> = if let Some(prefix) = pattern.strip_suffix("/*") {
            std::fs::read_dir(root.join(prefix))
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter(|e| e.path().is_dir())
                        .map(|e| join(prefix, &e.file_name().to_string_lossy()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![pattern.to_string()]
        };
        for dir in dirs {
            if let Some(name) = PackageJson::load(&root.join(&dir)).and_then(|p| p.name) {
                out.push((name, dir));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-project-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn write(c: &Cleanup, rel: &str, text: &str) {
        let p = c.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn strips_comments_and_trailing_commas_but_not_strings() {
        let src = "{\n  // line\n  \"a\": \"http://x/*y*/\", /* block */\n  \"b\": [1, 2,],\n}\n";
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], "http://x/*y*/");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn path_helpers() {
        assert_eq!(parent_dir("src/a/b.ts"), "src/a");
        assert_eq!(parent_dir("b.ts"), "");
        assert_eq!(join("", "x.ts"), "x.ts");
        assert_eq!(join("src", "./x.ts"), "src/./x.ts");
        assert_eq!(normalize("src/./a/../b.ts").as_deref(), Some("src/b.ts"));
        assert_eq!(normalize("./x").as_deref(), Some("x"));
        assert_eq!(normalize("../x"), None);
    }

    #[test]
    fn tsconfig_follows_extends_and_paths_base_rules() {
        let dir = fresh("tsconfig");
        write(&dir, "config/base.json", "{ \"compilerOptions\": { \"baseUrl\": \"..\" } }");
        write(
            &dir,
            "tsconfig.json",
            "{\n  \"extends\": \"./config/base.json\",\n  \"compilerOptions\": {\n    // alias\n    \"paths\": { \"@/*\": [\"src/*\"], \"lib\": [\"src/lib/index.ts\"], },\n  },\n}\n",
        );
        let cfg = TsConfig::load(&dir.0);
        assert_eq!(cfg.base_url.as_deref(), Some(""));
        assert_eq!(cfg.paths_base(), "");
        assert_eq!(
            cfg.paths,
            vec![("@/*".into(), vec!["src/*".into()]), ("lib".into(), vec!["src/lib/index.ts".into()])]
        );

        // Without a baseUrl anywhere, paths are relative to the declaring file's directory.
        let dir = fresh("tsconfig2");
        write(&dir, "tsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"~/*\": [\"./app/*\"] } } }");
        assert_eq!(TsConfig::load(&dir.0).paths_base(), "");

        let dir = fresh("tsconfig3");
        assert_eq!(TsConfig::load(&dir.0), TsConfig::default(), "no file means no aliases");
    }

    #[test]
    fn package_json_entry_files_and_workspaces() {
        let dir = fresh("pkg");
        write(
            &dir,
            "package.json",
            r#"{ "name": "root", "main": "src/index.ts", "bin": { "cli": "./bin/run.js" },
                "exports": { ".": { "import": "./src/index.ts", "types": "./dist/index.d.ts" }, "./sub": "./src/sub.ts" },
                "workspaces": ["packages/*", "tools"] }"#,
        );
        write(&dir, "packages/a/package.json", "{ \"name\": \"@x/a\", \"main\": \"index.ts\" }");
        write(&dir, "packages/b/package.json", "{ \"name\": \"@x/b\" }");
        write(&dir, "tools/package.json", "{ \"name\": \"tools\" }");
        let p = PackageJson::load(&dir.0).unwrap();
        assert_eq!(
            p.entry_files(""),
            vec!["src/index.ts", "bin/run.js", "src/index.ts", "dist/index.d.ts", "src/sub.ts"]
        );
        assert_eq!(
            workspace_packages(&dir.0, &p.workspaces),
            vec![
                ("@x/a".to_string(), "packages/a".to_string()),
                ("@x/b".to_string(), "packages/b".to_string()),
                ("tools".to_string(), "tools".to_string())
            ]
        );
        assert!(PackageJson::load(&dir.0.join("nowhere")).is_none());
    }
}
