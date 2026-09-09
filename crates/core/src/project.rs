//! The project files resolution and entry-point detection read: `tsconfig.json`
//! (aliases), `package.json` (entry points, jest setup files, workspaces),
//! `app.json` (Expo config plugins) and `wrangler.toml` (the deployed Worker
//! module). All are read leniently: a file that is missing contributes nothing,
//! and one that exists but does not parse warns once and contributes nothing
//! rather than failing the run.

use std::path::Path;

use serde_json::Value;

/// Reads a project file, dropping a leading byte order mark.
///
/// Editors on Windows write the mark into JSON, and no JSON or TOML parser
/// accepts it. Since every reader here is lenient about a file it cannot parse,
/// a marked tsconfig would quietly contribute no aliases at all and the engine
/// would report findings on live code.
fn read_project_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.strip_prefix('\u{feff}').map(String::from).unwrap_or(text))
}

/// Removes `//` and `/* */` comments and trailing commas outside strings, so a
/// tsconfig.json (which allows both) parses as JSON.
///
/// A comma is held back rather than emitted at once, because whether it is
/// trailing is only known at the next significant character: comments and the
/// whitespace around them may sit between the comma and a closing `}` or `]`.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    // A comma awaiting its verdict, and the whitespace seen since, replayed after
    // it so the stripped text keeps the original layout.
    let mut pending_comma = false;
    let mut pending_ws = String::new();
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
        // Comments emit nothing, so they leave a pending comma pending.
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c.is_whitespace() {
            if pending_comma {
                pending_ws.push(c);
            } else {
                out.push(c);
            }
            i += 1;
            continue;
        }
        if c == ',' {
            if pending_comma {
                out.push(',');
                out.push_str(&pending_ws);
                pending_ws.clear();
            }
            pending_comma = true;
            i += 1;
            continue;
        }
        if pending_comma {
            if !matches!(c, '}' | ']') {
                out.push(',');
            }
            out.push_str(&pending_ws);
            pending_ws.clear();
            pending_comma = false;
        }
        if c == '"' {
            in_string = true;
        }
        out.push(c);
        i += 1;
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
    /// `compilerOptions.paths`: pattern with at most one `*`, and its targets. Entries
    /// are sorted by key, not in declaration order, so the resolver must not depend on
    /// order and should match by longest prefix instead.
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
        let path = root.join(rel);
        let text = read_project_file(&path)?;
        let Ok(value) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else {
            eprintln!(
                "warning: {} could not be parsed; path aliases and entry points from it are ignored",
                path.display()
            );
            return None;
        };
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
    /// Files Jest loads by name from the `jest` block: `setupFiles`,
    /// `setupFilesAfterEnv`, `globalSetup` and `globalTeardown`, with a leading
    /// `<rootDir>/` removed. Bare package names are kept as they are and simply
    /// match no file in the repository.
    pub setup_files: Vec<String>,
    pub workspaces: Vec<String>,
}

/// The keys of the `jest` block whose values name a file the runner loads.
const JEST_SETUP_KEYS: &[&str] = &["setupFiles", "setupFilesAfterEnv", "globalSetup", "globalTeardown"];

fn strip_root_dir(p: &str) -> String {
    p.strip_prefix("<rootDir>/").unwrap_or(p).to_string()
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
        let path = dir.join("package.json");
        let text = read_project_file(&path)?;
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            eprintln!(
                "warning: {} could not be parsed; path aliases and entry points from it are ignored",
                path.display()
            );
            return None;
        };
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
        if let Some(jest) = v.get("jest") {
            for key in JEST_SETUP_KEYS {
                match jest.get(key) {
                    Some(Value::String(s)) => p.setup_files.push(strip_root_dir(s)),
                    Some(Value::Array(a)) => {
                        p.setup_files.extend(a.iter().filter_map(Value::as_str).map(strip_root_dir))
                    }
                    _ => {}
                }
            }
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
            .chain(self.setup_files.iter())
            .filter_map(|p| normalize(&join(dir_rel, p)))
            .collect()
    }
}

/// Expo config plugins that live in this repository, from `app.json` (or
/// `app.config.json`, the other plain-JSON spelling; `app.config.js` is code and
/// is not read). A plugin entry is either the path or `[path, options]`, and only
/// the `./` forms are files here: everything else is an installed package.
pub fn app_json_plugins(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["app.json", "app.config.json"] {
        let path = root.join(name);
        let Some(text) = read_project_file(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            eprintln!(
                "warning: {} could not be parsed; path aliases and entry points from it are ignored",
                path.display()
            );
            continue;
        };
        let plugins = v.get("expo").and_then(|e| e.get("plugins")).or_else(|| v.get("plugins"));
        let Some(entries) = plugins.and_then(Value::as_array) else { continue };
        for entry in entries {
            let path = match entry {
                Value::String(s) => Some(s.as_str()),
                Value::Array(a) => a.first().and_then(Value::as_str),
                _ => None,
            };
            match path.filter(|p| p.starts_with("./")).and_then(normalize) {
                Some(p) => out.push(p),
                None => continue,
            }
        }
    }
    out
}

/// The module a Cloudflare Worker deploys, from `wrangler.toml` or the `.jsonc`
/// spelling of the same file. Nothing in the repository imports it: the platform
/// loads it by name, and the rest of the code reaches it over HTTP.
pub fn wrangler_main(root: &Path) -> Option<String> {
    let toml_path = root.join("wrangler.toml");
    let main = match read_project_file(&toml_path) {
        Some(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(v) => v.get("main")?.as_str()?.to_string(),
            Err(_) => {
                eprintln!(
                    "warning: {} could not be parsed; path aliases and entry points from it are ignored",
                    toml_path.display()
                );
                return None;
            }
        },
        None => {
            let jsonc_path = root.join("wrangler.jsonc");
            let text = read_project_file(&jsonc_path)?;
            match serde_json::from_str::<Value>(&strip_jsonc(&text)) {
                Ok(v) => v.get("main")?.as_str()?.to_string(),
                Err(_) => {
                    eprintln!(
                        "warning: {} could not be parsed; path aliases and entry points from it are ignored",
                        jsonc_path.display()
                    );
                    return None;
                }
            }
        }
    };
    normalize(&main)
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

        // A trailing comma is still trailing when a comment sits between it and the
        // closing brace or bracket.
        let src = "{\n  \"paths\": {\n    \"@/*\": [\"src/*\"], // alias\n  },\n}\n";
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["paths"]["@/*"], serde_json::json!(["src/*"]));

        let v: Value = serde_json::from_str(&strip_jsonc("{ \"a\": 1, /* last */ }")).unwrap();
        assert_eq!(v["a"], 1);
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

    /// Editors write a byte order mark into JSON often enough that one cannot be
    /// allowed to quietly disable a repository's aliases and entry points: serde
    /// refuses the mark, and every reader here is lenient about a file it cannot
    /// parse, so the failure would show up only as findings on live code.
    #[test]
    fn a_byte_order_mark_does_not_hide_a_project_file() {
        let dir = fresh("bom");
        write(&dir, "tsconfig.json", "\u{feff}{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"src/*\"] } } }");
        assert_eq!(TsConfig::load(&dir.0).paths, vec![("@/*".to_string(), vec!["src/*".to_string()])]);

        write(&dir, "package.json", "\u{feff}{ \"name\": \"root\", \"main\": \"src/index.ts\" }");
        assert_eq!(PackageJson::load(&dir.0).and_then(|p| p.main).as_deref(), Some("src/index.ts"));
    }

    /// A file that exists but does not parse contributes nothing, as before. The
    /// warning it now prints goes to stderr and is not what this asserts.
    #[test]
    fn an_unparsable_tsconfig_contributes_nothing() {
        let dir = fresh("bad-tsconfig");
        write(&dir, "tsconfig.json", "{ \"compilerOptions\": { \"paths\": ");
        assert_eq!(TsConfig::load(&dir.0), TsConfig::default());
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

    #[test]
    fn jest_setup_files_and_app_json_plugins_are_read() {
        let dir = fresh("configs");
        write(
            &dir,
            "package.json",
            r#"{ "name": "root", "main": "src/index.ts",
                "jest": { "setupFiles": ["<rootDir>/jest-setup.js", "react-native-gesture-handler/jestSetup"],
                          "setupFilesAfterEnv": ["<rootDir>/jest-after.ts"],
                          "globalSetup": "./test/global.ts",
                          "globalTeardown": "<rootDir>/test/teardown.ts" } }"#,
        );
        let p = PackageJson::load(&dir.0).unwrap();
        assert_eq!(
            p.setup_files,
            vec![
                "jest-setup.js",
                "react-native-gesture-handler/jestSetup",
                "jest-after.ts",
                "./test/global.ts",
                "test/teardown.ts"
            ]
        );
        assert!(p.entry_files("").contains(&"test/global.ts".to_string()), "setup files are entry files");

        write(
            &dir,
            "app.json",
            r#"{ "expo": { "plugins": ["expo-font", "./plugins/withA", ["./plugins/withB.js", {}], 7] } }"#,
        );
        assert_eq!(
            app_json_plugins(&dir.0),
            vec!["plugins/withA", "plugins/withB.js"],
            "bare package names are not files"
        );

        let alt = fresh("configs-alt");
        write(&alt, "app.config.json", r#"{ "plugins": ["./plugins/withC"] }"#);
        assert_eq!(app_json_plugins(&alt.0), vec!["plugins/withC"]);

        let empty = fresh("configs-empty");
        assert!(app_json_plugins(&empty.0).is_empty(), "no app.json means no plugins");
    }

    #[test]
    fn wrangler_main_is_read_from_either_spelling() {
        let toml_dir = fresh("wrangler-toml");
        write(&toml_dir, "wrangler.toml", "name = \"media\"\nmain = \"./src/worker.ts\"\n");
        assert_eq!(wrangler_main(&toml_dir.0).as_deref(), Some("src/worker.ts"), "leading ./ is stripped");

        let jsonc_dir = fresh("wrangler-jsonc");
        write(&jsonc_dir, "wrangler.jsonc", "{\n  // the deployed module\n  \"main\": \"src/index.ts\",\n}\n");
        assert_eq!(wrangler_main(&jsonc_dir.0).as_deref(), Some("src/index.ts"));

        let empty = fresh("wrangler-none");
        assert_eq!(wrangler_main(&empty.0), None, "no wrangler config means no entry");
        write(&empty, "wrangler.toml", "name = \"no-main\"\n");
        assert_eq!(wrangler_main(&empty.0), None, "a config without main names no file");
    }
}
