//! The npm, yarn and pnpm lockfile parsers.
//!
//! One repository is described by at most one lockfile, so [`read`] picks the
//! first of `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml` that exists and
//! stops. Every parser answers the same question, "which package versions does
//! an install of this repository put on disk", and answers it by text so a
//! finding can point at the line the package is declared on.
//!
//! Nothing here validates the lockfile. A file the parser cannot make sense of
//! yields an empty package list rather than an error: a lockfile is an input to
//! an advisory lookup, not a source file the run is grading, and spec 9 says a
//! problem with an input like that must not fail the run.

use std::collections::HashMap;
use std::path::Path;

use crate::index::content_hash;

/// One installed package: the name the registry knows it by, the exact version
/// on disk, and the 1-based line of the lockfile entry that declares it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub line: u32,
}

/// A parsed lockfile: its repo-relative path, a hash of the package list that
/// keys the advisory snapshot, and its packages deduplicated by (name, version)
/// and sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lockfile {
    pub rel: String,
    /// Identifies the package list, not the file. The advisory snapshot this
    /// keys is zipped positionally against `packages`, so what the key has to
    /// promise is that the list is the same one the snapshot answered for. A
    /// text hash would promise less than that (a change to this module's
    /// parsers would keep the key and move the list, attaching advisories to
    /// the wrong package) and more than needed (a resolved-URL edit that leaves
    /// every version alone would throw the snapshot away).
    pub hash: String,
    pub packages: Vec<Package>,
}

/// A parser: the whole lockfile's text in, its packages out.
type Parser = fn(&str) -> Vec<Package>;

/// The lockfiles this engine reads, in the order it prefers them.
const CANDIDATES: [(&str, Parser); 3] =
    [("package-lock.json", parse_npm), ("yarn.lock", parse_yarn), ("pnpm-lock.yaml", parse_pnpm)];

/// The lockfile at the root of `root`, or `None` when the repository has none.
///
/// A lockfile that exists but cannot be read, because a permission denies it or
/// because it is not UTF-8, is `None` and a warning rather than an error: the
/// module doc's rule is that a problem with this input never fails the run, and
/// a file the parser could not have made sense of anyway is exactly that.
pub fn read(root: &Path) -> anyhow::Result<Option<Lockfile>> {
    for (name, parse) in CANDIDATES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}; dependency advisories skipped", path.display());
                return Ok(None);
            }
        };
        let packages = normalize(parse(&text));
        return Ok(Some(Lockfile { rel: name.to_string(), hash: package_hash(&packages), packages }));
    }
    Ok(None)
}

/// The key a snapshot of these packages is stored under: `name@version` per
/// line, sorted, hashed. See [`Lockfile::hash`].
fn package_hash(packages: &[Package]) -> String {
    let mut lines: Vec<String> = packages.iter().map(|p| format!("{}@{}", p.name, p.version)).collect();
    // `normalize` already sorted by name then version then line, which orders
    // these the same way, but the hash does not depend on a caller having done
    // that: the point of the key is that one list has one spelling.
    lines.sort();
    content_hash(&lines.join("\n"))
}

/// Deduplicates by (name, version), keeping the earliest line, and sorts.
///
/// A lockfile lists the same version of a transitive dependency once per place
/// it is hoisted to; one advisory finding per version is what a reader wants,
/// and the earliest line is the one nearest the top of the file.
fn normalize(mut packages: Vec<Package>) -> Vec<Package> {
    packages.sort();
    packages.dedup_by(|a, b| a.name == b.name && a.version == b.version);
    packages
}

/// `package-lock.json` at lockfileVersion 2 or 3: the `packages` map, keyed by
/// install path.
///
/// A key is a path, not a name, so the name is the segment after the last
/// `node_modules/`: `node_modules/a/node_modules/b` is package `b` installed
/// under `a`. The `""` key is the repository itself and is skipped, and a
/// `link: true` entry is a workspace symlink whose real entry appears elsewhere
/// in the map.
fn parse_npm(text: &str) -> Vec<Package> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let Some(packages) = root.get("packages").and_then(|p| p.as_object()) else { return Vec::new() };
    let lines = key_lines(text);
    let mut out = Vec::new();
    for (key, entry) in packages {
        if key.is_empty() || entry.get("link").and_then(|l| l.as_bool()).unwrap_or(false) {
            continue;
        }
        // A key without `node_modules/` is a workspace directory, not an install.
        let Some((_, name)) = key.rsplit_once("node_modules/") else { continue };
        let Some(version) = entry.get("version").and_then(|v| v.as_str()) else { continue };
        if name.is_empty() || version.is_empty() {
            continue;
        }
        out.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            line: lines.get(key.as_str()).copied().unwrap_or(1),
        });
    }
    out
}

/// `yarn.lock` v1: a block header naming one or more ranges, then an indented
/// `version "<v>"`.
///
/// Every range in a header resolved to the same install, so the first one names
/// the package. The name is everything before the last `@` that is not at index
/// zero, which keeps the leading `@` of a scoped name.
fn parse_yarn(text: &str) -> Vec<Package> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.starts_with('#') || line.starts_with(char::is_whitespace) || !line.trim_end().ends_with(':') {
            continue;
        }
        let header = line.trim_end().trim_end_matches(':');
        let first = header.split(',').next().unwrap_or_default().trim().trim_matches('"');
        let Some(name) = split_name(first).map(|(name, _)| name) else { continue };
        // The block runs until the first line that is not indented, which is the
        // blank line yarn writes between blocks.
        let Some(version) = lines[idx + 1..]
            .iter()
            .take_while(|l| l.starts_with(char::is_whitespace))
            .find_map(|l| l.trim().strip_prefix("version"))
            .map(|v| v.trim().trim_start_matches(':').trim().trim_matches('"'))
        else {
            continue;
        };
        if version.is_empty() {
            continue;
        }
        out.push(Package { name: name.to_string(), version: version.to_string(), line: idx as u32 + 1 });
    }
    out
}

/// `pnpm-lock.yaml`: the entries directly under the top-level `packages:` key.
///
/// v6 writes `  /<name>@<version>:` and v9 writes `  <name>@<version>:`, either
/// of them quoted when the name is scoped, and either of them followed by
/// `(peer@version)` suffixes recording which peer resolution this copy is for.
/// The `importers:` and `snapshots:` sections repeat the same keys, so only the
/// `packages:` section is read.
fn parse_pnpm(text: &str) -> Vec<Package> {
    let mut out = Vec::new();
    let mut in_packages = false;
    for (idx, line) in text.lines().enumerate() {
        if !line.is_empty() && !line.starts_with(' ') {
            in_packages = line.trim_end() == "packages:";
            continue;
        }
        if !in_packages || line.len() - line.trim_start().len() != 2 {
            continue;
        }
        let entry = line.trim().trim_end_matches("{}").trim_end();
        let Some(entry) = entry.strip_suffix(':') else { continue };
        let entry = entry.trim().trim_matches('\'').trim_matches('"');
        let entry = entry.strip_prefix('/').unwrap_or(entry);
        let entry = entry.split_once('(').map(|(head, _)| head).unwrap_or(entry);
        let Some((name, version)) = split_name(entry) else { continue };
        if version.is_empty() {
            continue;
        }
        out.push(Package { name: name.to_string(), version: version.to_string(), line: idx as u32 + 1 });
    }
    out
}

/// Splits `<name>@<rest>` at the last `@` that is not at index zero, so a scoped
/// name keeps the `@` it starts with.
fn split_name(spec: &str) -> Option<(&str, &str)> {
    let at = spec.rfind('@').filter(|&at| at > 0)?;
    Some((&spec[..at], &spec[at + 1..]))
}

/// The first line each top-level-quoted JSON key appears on, so a package parsed
/// out of `serde_json` (which does not record positions) can still be pointed at.
fn key_lines(text: &str) -> HashMap<&str, u32> {
    let mut lines = HashMap::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('"') else { continue };
        let Some(end) = rest.find('"') else { continue };
        lines.entry(&rest[..end]).or_insert(idx as u32 + 1);
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lockfiles")
    }

    fn text(name: &str) -> String {
        std::fs::read_to_string(fixtures().join(name)).unwrap()
    }

    /// The package list every fixture describes, with the line each entry sits on.
    fn expected(lines: [u32; 4]) -> Vec<Package> {
        let names = [("@acme/util", "2.0.0"), ("b", "3.1.0"), ("left-pad", "1.3.0"), ("lodash", "4.17.15")];
        names
            .iter()
            .zip(lines)
            .map(|((name, version), line)| Package { name: name.to_string(), version: version.to_string(), line })
            .collect()
    }

    #[test]
    fn npm_takes_the_segment_after_the_last_node_modules_and_skips_the_root_and_links() {
        assert_eq!(normalize(parse_npm(&text("package-lock.json"))), expected([16, 20, 24, 32]));
    }

    #[test]
    fn yarn_takes_the_name_from_the_first_range_in_a_block_header() {
        assert_eq!(normalize(parse_yarn(&text("yarn.lock"))), expected([5, 9, 13, 17]));
    }

    #[test]
    fn pnpm_strips_peer_suffixes_and_reads_only_the_packages_section() {
        assert_eq!(normalize(parse_pnpm(&text("pnpm-lock.yaml"))), expected([16, 19, 22, 25]));
    }

    #[test]
    fn the_three_parsers_agree_on_the_package_list() {
        let strip = |packages: Vec<Package>| -> Vec<(String, String)> {
            packages.into_iter().map(|p| (p.name, p.version)).collect()
        };
        let npm = strip(normalize(parse_npm(&text("package-lock.json"))));
        assert_eq!(npm, strip(normalize(parse_yarn(&text("yarn.lock")))));
        assert_eq!(npm, strip(normalize(parse_pnpm(&text("pnpm-lock.yaml")))));
    }

    #[test]
    fn read_prefers_the_npm_lockfile_and_hashes_its_package_list() {
        let lock = read(&fixtures()).unwrap().expect("the fixture directory has lockfiles");
        assert_eq!(lock.rel, "package-lock.json");
        assert_eq!(lock.hash, package_hash(&expected([16, 20, 24, 32])));
        assert_ne!(lock.hash, content_hash(&text("package-lock.json")), "the text is not what is hashed");
        assert_eq!(lock.packages, expected([16, 20, 24, 32]));
    }

    /// The hash keys a snapshot that is zipped positionally against the package
    /// list, so it has to answer for the list and not for the text that produced
    /// it: three lockfile formats describing one install share it, and a fourth
    /// package changes it.
    #[test]
    fn the_hash_follows_the_package_list_and_not_the_text() {
        let npm = package_hash(&normalize(parse_npm(&text("package-lock.json"))));
        let yarn = package_hash(&normalize(parse_yarn(&text("yarn.lock"))));
        let pnpm = package_hash(&normalize(parse_pnpm(&text("pnpm-lock.yaml"))));
        assert_eq!(npm, yarn, "the same packages at different lines hash the same");
        assert_eq!(npm, pnpm);

        let mut more = normalize(parse_npm(&text("package-lock.json")));
        more.push(Package { name: "extra".into(), version: "1.0.0".into(), line: 99 });
        assert_ne!(npm, package_hash(&normalize(more)), "one more package is a different list");
    }

    #[test]
    fn read_finds_nothing_in_a_directory_without_a_lockfile() {
        let dir = std::env::temp_dir().join(format!("locrin-lockfile-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lockfile that is not text is an unreadable input, not a broken run.
    #[test]
    fn a_lockfile_that_is_not_utf8_reads_as_no_lockfile_rather_than_an_error() {
        let dir = std::env::temp_dir().join(format!("locrin-lockfile-binary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package-lock.json"), [0x7b, 0xff, 0xfe, 0x7d]).unwrap();
        assert!(read(&dir).unwrap().is_none(), "an unreadable lockfile is None, and the run carries on");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lockfile_that_does_not_parse_yields_no_packages_rather_than_an_error() {
        assert!(parse_npm("{ not json").is_empty());
        assert!(parse_yarn("").is_empty());
        assert!(parse_pnpm("packages:\n").is_empty());
    }

    #[test]
    fn duplicate_versions_collapse_to_the_earliest_line() {
        let dupes = vec![
            Package { name: "lodash".into(), version: "4.17.15".into(), line: 40 },
            Package { name: "lodash".into(), version: "4.17.15".into(), line: 12 },
        ];
        assert_eq!(normalize(dupes), vec![Package { name: "lodash".into(), version: "4.17.15".into(), line: 12 }]);
    }
}
