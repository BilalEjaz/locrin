//! Files a framework, a runner, or a package consumer loads by convention rather
//! than by an import statement. The graph cannot see those loads, so these files
//! and their exports are never reported dead (spec 4.1).

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::project::{app_json_plugins, workspace_packages, wrangler_main, PackageJson};

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
    // Serverless functions: deployed by directory name and called over HTTP, so
    // nothing in the repository ever imports them. Only the deployed module
    // itself, never the whole tree: a `_shared` directory beside it is ordinary
    // code, and exempting it costs real `dead-export` findings.
    "**/functions/*/index.*",
    // Expo config plugins, named as strings in app.json. A plugin named by path
    // is picked up from app.json instead, so no blanket `plugins/**` here: a
    // repository whose `plugins/` holds application code keeps its findings.
    "**/*.plugin.*",
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
        files.extend(app_json_plugins(root).iter().map(|f| strip_ext(f)));
        files.extend(wrangler_main(root).iter().map(|f| strip_ext(f)));
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

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-entry-{tag}-{}", std::process::id()));
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
    fn deployed_functions_and_config_plugins_are_entries_by_glob() {
        let e = EntryPoints::detect(&mini(), &[]).unwrap();
        assert!(e.is_entry("supabase/functions/challenge-create/index.ts"), "deployed by directory name");
        assert!(
            !e.is_entry("supabase/functions/_shared/cors.ts"),
            "shared code beside a deployed function is ordinary code, and exempting it hides dead exports"
        );
        assert!(e.is_entry("server/functions/notify/index.js"), "the same convention outside supabase/");
        assert!(e.is_entry("src/build/withFoo.plugin.ts"));
        assert!(
            !e.is_entry("plugins/withHealthConnectManifest.js"),
            "a config plugin is an entry because app.json names it, not because of where it sits"
        );
        assert!(!e.is_entry("src/functions/helper.ts"), "only functions/<name>/index.* is a deployed entry");
    }

    #[test]
    fn app_json_plugins_and_jest_setup_files_are_entries() {
        let dir = fresh("config-entries");
        write(
            &dir,
            "app.json",
            r#"{ "expo": { "plugins": ["expo-router", "./tools/withA", ["./tools/withB.js", { "mode": "strict" }]] } }"#,
        );
        write(
            &dir,
            "package.json",
            r#"{ "name": "x", "jest": { "setupFiles": ["<rootDir>/jest-setup.js"], "globalSetup": "./test/global.ts" } }"#,
        );
        let e = EntryPoints::detect(&dir.0, &[]).unwrap();
        assert!(e.is_entry("tools/withA.js"), "app.json plugin named without an extension");
        assert!(e.is_entry("tools/withB.js"), "app.json plugin named as [path, options]");
        assert!(e.is_entry("jest-setup.js"), "jest.setupFiles, with <rootDir> stripped");
        assert!(e.is_entry("test/global.ts"), "jest.globalSetup");
        assert!(!e.is_entry("tools/other.js"), "a file no config names is not an entry");
    }

    #[test]
    fn the_deployed_worker_module_is_an_entry() {
        let dir = fresh("wrangler");
        write(&dir, "wrangler.toml", "name = \"media\"\nmain = \"./src/worker.ts\"\n");
        let e = EntryPoints::detect(&dir.0, &[]).unwrap();
        assert!(e.is_entry("src/worker.ts"), "wrangler.toml main");
        assert!(!e.is_entry("src/helper.ts"), "only the module wrangler names");
    }

    #[test]
    fn strip_ext_handles_dots_in_directories() {
        assert_eq!(strip_ext("src/index.ts"), "src/index");
        assert_eq!(strip_ext("a.b/index"), "a.b/index");
        assert_eq!(strip_ext("index.ts"), "index");
    }
}
