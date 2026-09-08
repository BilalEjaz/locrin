//! Files a framework, a runner, or a package consumer loads by convention rather
//! than by an import statement. The graph cannot see those loads, so these files
//! and their exports are never reported dead (spec 4.1).

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::project::{workspace_packages, PackageJson};

pub const DEFAULT_ENTRY_GLOBS: &[&str] = &[
    // Next.js and Expo Router load routes from these folders.
    "app/**",
    "pages/**",
    "src/app/**",
    "src/pages/**",
    // Conventional roots.
    "index.*",
    "src/index.*",
    "App.*",
    "src/App.*",
    "main.*",
    "src/main.*",
    // Test runners, storybook, mocks.
    "**/*.test.*",
    "**/*.spec.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/*.stories.*",
    // Tooling executes these directly.
    "**/*.config.*",
    "**/*.setup.*",
    "**/scripts/**",
    "**/bin/**",
];

#[derive(Debug)]
pub struct EntryPoints {
    globs: GlobSet,
    /// Extension-less repo-relative paths named by package.json files.
    files: HashSet<String>,
}

/// `src/index.ts` and `src/index.js` are one entry as far as package.json is concerned.
fn strip_ext(rel: &str) -> String {
    match (rel.rfind('/'), rel.rfind('.')) {
        (Some(slash), Some(dot)) if dot > slash => rel[..dot].to_string(),
        (None, Some(dot)) => rel[..dot].to_string(),
        _ => rel.to_string(),
    }
}

impl EntryPoints {
    pub fn detect(root: &Path, extra: &[String]) -> anyhow::Result<EntryPoints> {
        let mut b = GlobSetBuilder::new();
        for g in DEFAULT_ENTRY_GLOBS.iter().map(|s| s.to_string()).chain(extra.iter().cloned()) {
            b.add(Glob::new(&g).with_context(|| format!("entry_points contains an invalid glob: {g}"))?);
        }
        let mut files = HashSet::new();
        if let Some(pkg) = PackageJson::load(root) {
            files.extend(pkg.entry_files("").iter().map(|f| strip_ext(f)));
            for (_, dir) in workspace_packages(root, &pkg.workspaces) {
                if let Some(p) = PackageJson::load(&root.join(&dir)) {
                    files.extend(p.entry_files(&dir).iter().map(|f| strip_ext(f)));
                }
            }
        }
        Ok(EntryPoints { globs: b.build()?, files })
    }

    pub fn is_entry(&self, rel: &str) -> bool {
        self.globs.is_match(rel) || self.files.contains(&strip_ext(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mini() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
    }

    #[test]
    fn package_main_and_default_globs_are_entries() {
        let e = EntryPoints::detect(&mini(), &[]).unwrap();
        assert!(e.is_entry("src/index.ts"), "package.json main");
        assert!(e.is_entry("src/index.js"), "extension does not matter for package.json entries");
        assert!(e.is_entry("app/(tabs)/home.tsx"));
        assert!(e.is_entry("src/lib/a.test.ts"));
        assert!(e.is_entry("jest.config.ts"));
        assert!(e.is_entry("scripts/build.ts"));
        assert!(!e.is_entry("src/util.ts"));
        assert!(!e.is_entry("src/config.ts"), "config.ts is not *.config.*");
    }

    #[test]
    fn extra_globs_extend_and_bad_globs_fail_loudly() {
        let e = EntryPoints::detect(&mini(), &["tools/**".into()]).unwrap();
        assert!(e.is_entry("tools/run.ts"));
        let err = format!("{:#}", EntryPoints::detect(&mini(), &["tools/[".into()]).unwrap_err());
        assert!(err.contains("tools/["), "{err}");
    }

    #[test]
    fn strip_ext_handles_dots_in_directories() {
        assert_eq!(strip_ext("src/index.ts"), "src/index");
        assert_eq!(strip_ext("a.b/index"), "a.b/index");
        assert_eq!(strip_ext("index.ts"), "index");
    }
}
