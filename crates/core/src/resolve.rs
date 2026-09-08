//! Heuristic module resolution (spec 3.2). Relative specifiers, tsconfig `paths`
//! and `baseUrl`, workspace packages, and the extension probing TypeScript does.
//! Never guesses: what it cannot find on disk is `Unresolved`, what it finds on
//! disk but does not index is `External`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::project::{join, normalize, parent_dir, workspace_packages, PackageJson, TsConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A repo-relative path of an indexed source file.
    Resolved(String),
    /// A package, a declaration file, an asset, or an excluded file: real, but not a node in this graph.
    External,
    /// Nothing on disk matches. Recorded as such, never guessed.
    Unresolved,
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Resolved(_) => "resolved",
            Resolution::External => "external",
            Resolution::Unresolved => "unresolved",
        }
    }
}

const SOURCE_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

/// TypeScript lets a specifier carry the *output* extension of a source file.
const OUTPUT_TO_SOURCE: &[(&str, &str)] =
    &[(".js", ".ts"), (".js", ".tsx"), (".jsx", ".tsx"), (".mjs", ".mts"), (".cjs", ".cts")];

pub struct Resolver {
    root: PathBuf,
    indexed: HashSet<String>,
    tsconfig: TsConfig,
    packages: Vec<(String, String)>,
}

/// Matches a tsconfig `paths` pattern (at most one `*`) and returns the text the star stood for.
fn match_pattern(pattern: &str, specifier: &str) -> Option<String> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            if specifier.len() >= prefix.len() + suffix.len()
                && specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
            {
                Some(specifier[prefix.len()..specifier.len() - suffix.len()].to_string())
            } else {
                None
            }
        }
        None => (pattern == specifier).then(String::new),
    }
}

/// The literal text a pattern requires before its `*`, or the whole pattern when it has none.
/// TypeScript gives an ambiguous specifier to the longest such prefix whatever the declaration
/// order, and `TsConfig::paths` arrives sorted by key rather than in that order.
/// A TypeScript declaration file: it types a module rather than being one.
fn is_declaration(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}

fn literal_prefix_len(pattern: &str) -> usize {
    match pattern.split_once('*') {
        Some((prefix, _)) => prefix.len(),
        None => pattern.len(),
    }
}

impl Resolver {
    pub fn new(root: &Path, indexed: HashSet<String>) -> Resolver {
        let mut tsconfig = TsConfig::load(root);
        tsconfig.paths.sort_by_key(|(pattern, _)| std::cmp::Reverse(literal_prefix_len(pattern)));
        let packages = PackageJson::load(root).map(|p| workspace_packages(root, &p.workspaces)).unwrap_or_default();
        Resolver { root: root.to_path_buf(), indexed, tsconfig, packages }
    }

    pub fn resolve(&self, from_rel: &str, specifier: &str) -> Resolution {
        if specifier.starts_with('.') {
            return match normalize(&join(&parent_dir(from_rel), specifier)) {
                Some(p) => self.probe(&p),
                None => Resolution::Unresolved,
            };
        }
        if specifier.starts_with('/') {
            // An absolute path names a machine, not a repository.
            return Resolution::Unresolved;
        }
        let mut aliased: Option<Resolution> = None;
        for (pattern, targets) in &self.tsconfig.paths {
            let Some(rest) = match_pattern(pattern, specifier) else { continue };
            for target in targets {
                let candidate = target.replacen('*', &rest, 1);
                let Some(p) = normalize(&join(self.tsconfig.paths_base(), &candidate)) else { continue };
                match self.probe(&p) {
                    Resolution::Resolved(r) => return Resolution::Resolved(r),
                    Resolution::External => aliased = Some(Resolution::External),
                    Resolution::Unresolved => {
                        aliased.get_or_insert(Resolution::Unresolved);
                    }
                }
            }
        }
        if let Some(r) = aliased {
            // An alias matched but no target is an indexed file: the alias points
            // at a declaration or an asset (External) or at nothing (Unresolved).
            return r;
        }
        for (name, dir) in &self.packages {
            if specifier == name {
                let main =
                    PackageJson::load(&self.root.join(dir)).and_then(|p| p.main).unwrap_or_else(|| "index".into());
                return normalize(&join(dir, &main)).map(|p| self.probe(&p)).unwrap_or(Resolution::Unresolved);
            }
            if let Some(sub) = specifier.strip_prefix(&format!("{name}/")) {
                return normalize(&join(dir, sub)).map(|p| self.probe(&p)).unwrap_or(Resolution::Unresolved);
            }
        }
        // A bare specifier that no alias or workspace owns is a package. It cannot
        // name a file in this repository, so External is a fact, not a guess.
        Resolution::External
    }

    /// Every path TypeScript would accept for `p`: the literal path, the source behind an
    /// output extension, `p` plus each source extension, and `index.<ext>` inside `p`.
    fn candidates(&self, p: &str) -> Vec<String> {
        let mut candidates = vec![p.to_string()];
        for (output, source) in OUTPUT_TO_SOURCE {
            if let Some(stem) = p.strip_suffix(output) {
                candidates.push(format!("{stem}{source}"));
            }
        }
        for ext in SOURCE_EXTS {
            candidates.push(format!("{p}.{ext}"));
        }
        for ext in SOURCE_EXTS {
            candidates.push(format!("{p}/index.{ext}"));
        }
        candidates
    }

    /// The first candidate the caller indexed, ignoring declaration files: a `.d.ts` describes a
    /// module, it is not one, so it is never a node in this graph however it reached `indexed`.
    fn indexed_hit(&self, candidates: &[String]) -> Option<String> {
        candidates.iter().find(|c| !is_declaration(c) && self.indexed.contains(c.as_str())).cloned()
    }

    fn probe(&self, p: &str) -> Resolution {
        let candidates = self.candidates(p);
        if let Some(hit) = self.indexed_hit(&candidates) {
            return Resolution::Resolved(hit);
        }
        let on_disk = self.root.join(p);
        if on_disk.join("package.json").is_file() {
            // A directory that carries a package file states its own entry point. Resolve that
            // entry once and stop: a `main` naming the directory again must not recurse.
            if let Some(main) = PackageJson::load(&on_disk).and_then(|pkg| pkg.main) {
                if let Some(target) = normalize(&join(p, &main)) {
                    return match self.indexed_hit(&self.candidates(&target)) {
                        Some(hit) => Resolution::Resolved(hit),
                        None => Resolution::External,
                    };
                }
            }
        }
        let exists = candidates.iter().any(|c| self.root.join(c).is_file())
            || on_disk.join("package.json").is_file()
            || on_disk.join("index.d.ts").is_file()
            || self.root.join(format!("{p}.d.ts")).is_file();
        if exists {
            Resolution::External
        } else {
            Resolution::Unresolved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-resolve-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn write(c: &Cleanup, rel: &str, text: &str) {
        let p = c.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn resolver(c: &Cleanup, indexed: &[&str]) -> Resolver {
        Resolver::new(&c.0, indexed.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn relative_specifiers_probe_extensions_and_index_files() {
        let dir = fresh("relative");
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.tsx", "");
        write(&dir, "src/c/index.ts", "");
        write(&dir, "src/gen/x.ts", "");
        write(&dir, "src/gen/index.ts", "");
        write(&dir, "src/x.ts", "");
        write(&dir, "src/types.d.ts", "");
        write(&dir, "src/logo.png", "");
        let r = resolver(&dir, &["src/a.ts", "src/b.tsx", "src/c/index.ts", "src/main.ts"]);
        assert_eq!(r.resolve("src/main.ts", "./a"), Resolution::Resolved("src/a.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "./a.js"),
            Resolution::Resolved("src/a.ts".into()),
            "output extension names the source"
        );
        assert_eq!(r.resolve("src/main.ts", "./b"), Resolution::Resolved("src/b.tsx".into()));
        assert_eq!(r.resolve("src/main.ts", "./c"), Resolution::Resolved("src/c/index.ts".into()));
        assert_eq!(r.resolve("src/c/index.ts", "../a"), Resolution::Resolved("src/a.ts".into()));
        assert_eq!(r.resolve("src/main.ts", "./gen/x"), Resolution::External, "on disk but not indexed (excluded)");
        assert_eq!(
            r.resolve("src/main.ts", "./gen"),
            Resolution::External,
            "an index file on disk but not indexed is external, not unresolved"
        );
        assert_eq!(
            r.resolve("src/main.ts", "./x.js"),
            Resolution::External,
            "the source behind an output extension is on disk but not indexed"
        );
        assert_eq!(r.resolve("src/main.ts", "./types"), Resolution::External, "declaration file");
        assert_eq!(r.resolve("src/main.ts", "./logo.png"), Resolution::External, "asset");
        assert_eq!(r.resolve("src/main.ts", "./missing"), Resolution::Unresolved);
        assert_eq!(r.resolve("src/main.ts", "../../escape"), Resolution::Unresolved);
        assert_eq!(r.resolve("src/main.ts", "/etc/x"), Resolution::Unresolved);
    }

    #[test]
    fn aliases_workspaces_and_bare_specifiers() {
        let dir = fresh("alias");
        write(
            &dir,
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"], \"types\": [\"src/types.d.ts\"] } } }",
        );
        write(&dir, "package.json", "{ \"name\": \"root\", \"workspaces\": [\"packages/*\"] }");
        write(&dir, "packages/ui/package.json", "{ \"name\": \"@acme/ui\", \"main\": \"src/index.ts\" }");
        write(&dir, "packages/ui/src/index.ts", "");
        write(&dir, "packages/ui/src/button.tsx", "");
        write(&dir, "src/util.ts", "");
        write(&dir, "src/types.d.ts", "");
        let r =
            resolver(&dir, &["src/util.ts", "packages/ui/src/index.ts", "packages/ui/src/button.tsx", "src/main.ts"]);
        assert_eq!(r.resolve("src/main.ts", "@/util"), Resolution::Resolved("src/util.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "@/nope"),
            Resolution::Unresolved,
            "an alias that points at nothing is unresolved, not external"
        );
        assert_eq!(r.resolve("src/main.ts", "types"), Resolution::External);
        assert_eq!(r.resolve("src/main.ts", "@acme/ui"), Resolution::Resolved("packages/ui/src/index.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "@acme/ui/src/button"),
            Resolution::Resolved("packages/ui/src/button.tsx".into())
        );
        assert_eq!(r.resolve("src/main.ts", "react"), Resolution::External);
        assert_eq!(r.resolve("src/main.ts", "node:fs"), Resolution::External);
    }

    #[test]
    fn the_longest_alias_prefix_wins_whatever_the_key_order() {
        let dir = fresh("overlap");
        write(
            &dir,
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"], \"@/ui/*\": [\"packages/ui/*\"] } } }",
        );
        write(&dir, "src/ui/button.ts", "");
        write(&dir, "packages/ui/button.ts", "");
        let r = resolver(&dir, &["src/ui/button.ts", "packages/ui/button.ts", "src/main.ts"]);
        assert_eq!(
            r.resolve("src/main.ts", "@/ui/button"),
            Resolution::Resolved("packages/ui/button.ts".into()),
            "the more specific alias owns the specifier"
        );
        assert_eq!(r.resolve("src/main.ts", "@/other"), Resolution::Unresolved);
    }

    #[test]
    fn declaration_files_are_never_resolved() {
        let dir = fresh("decl");
        write(&dir, "src/types.d.ts", "");
        let r = resolver(&dir, &["src/types.d.ts", "src/main.ts"]);
        assert_eq!(r.resolve("src/main.ts", "./types"), Resolution::External);
        assert_eq!(
            r.resolve("src/main.ts", "./types.d.ts"),
            Resolution::External,
            "a declaration file is never a node in this graph, whatever the caller indexed"
        );
    }

    #[test]
    fn a_directory_with_a_package_json_resolves_through_its_main() {
        let dir = fresh("dirmain");
        write(&dir, "packages/ui/package.json", "{ \"name\": \"@acme/ui\", \"main\": \"src/index.ts\" }");
        write(&dir, "packages/ui/src/index.ts", "");
        write(&dir, "packages/lib/package.json", "{ \"name\": \"@acme/lib\", \"main\": \"src/index.ts\" }");
        write(&dir, "packages/lib/src/index.ts", "");
        let r = resolver(&dir, &["packages/ui/src/index.ts", "src/main.ts"]);
        assert_eq!(
            r.resolve("src/main.ts", "../packages/ui"),
            Resolution::Resolved("packages/ui/src/index.ts".into()),
            "the package file states the entry point"
        );
        assert_eq!(
            r.resolve("src/main.ts", "../packages/lib"),
            Resolution::External,
            "an entry point on disk but not indexed is external"
        );
    }

    #[test]
    fn pattern_matching() {
        assert_eq!(match_pattern("@/*", "@/a/b").as_deref(), Some("a/b"));
        assert_eq!(match_pattern("*", "x").as_deref(), Some("x"));
        assert_eq!(match_pattern("lib", "lib").as_deref(), Some(""));
        assert_eq!(match_pattern("lib", "lib/x"), None);
        assert_eq!(match_pattern("@/*", "src/x"), None);
        assert_eq!(literal_prefix_len("@/ui/*"), 5);
        assert_eq!(literal_prefix_len("types"), 5);
    }
}
