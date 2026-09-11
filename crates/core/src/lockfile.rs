//! The lockfile parsers: npm, yarn and pnpm for JavaScript, Composer for PHP,
//! Poetry and `requirements.txt` for Python.
//!
//! A repository can hold several at once, and a polyglot one usually does, so
//! [`read`] answers with every lockfile that exists rather than with the first.
//! Each is its own question to the advisory database: `requests` is a PyPI
//! package and an npm package with unrelated versions and unrelated advisories,
//! so every package carries the [`Package::ecosystem`] it was installed from and
//! every lockfile keys its own snapshot.
//!
//! Every parser answers the same question, "which package versions does an
//! install of this repository put on disk", and answers it by text so a finding
//! can point at the line the package is declared on.
//!
//! Nothing here validates the lockfile. A file the parser cannot make sense of
//! yields an empty package list rather than an error: a lockfile is an input to
//! an advisory lookup, not a source file the run is grading, and spec 9 says a
//! problem with an input like that must not fail the run.

use std::collections::HashMap;
use std::path::Path;

use crate::index::content_hash;

/// The three ecosystem names this engine queries, spelled the way osv.dev
/// spells them: the string goes into the request and is compared against the
/// advisory's own, so it is the registry's spelling and not this engine's.
pub const NPM: &str = "npm";
pub const PACKAGIST: &str = "Packagist";
pub const PYPI: &str = "PyPI";

/// One installed package: the name the registry knows it by, the exact version
/// on disk, the 1-based line of the lockfile entry that declares it, and the
/// registry it came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub line: u32,
    /// [`NPM`], [`PACKAGIST`] or [`PYPI`]. Two registries can hold one name at
    /// one version and mean two different pieces of software, so this is half of
    /// the package's identity and not a label on it: it is sent with the query
    /// and required to match on the way back.
    pub ecosystem: &'static str,
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
    ///
    /// The ecosystem is in it for the same reason the list is: a repository with
    /// a `package-lock.json` and a `requirements.txt` asks two questions, and an
    /// npm snapshot must never be served as the answer to the PyPI one.
    pub hash: String,
    pub packages: Vec<Package>,
}

/// A parser: the whole lockfile's text in, its packages out.
type Parser = fn(&str) -> Vec<Package>;

/// The lockfiles this engine reads, in the order it reports them. The ecosystem
/// is the one every package the parser beside it produces carries, and
/// `every_parser_agrees_with_the_ecosystem_beside_it` holds the two together.
const CANDIDATES: [(&str, &str, Parser); 6] = [
    ("package-lock.json", NPM, parse_npm),
    ("yarn.lock", NPM, parse_yarn),
    ("pnpm-lock.yaml", NPM, parse_pnpm),
    ("composer.lock", PACKAGIST, parse_composer),
    ("poetry.lock", PYPI, parse_poetry),
    ("requirements.txt", PYPI, parse_requirements),
];

/// Bumped whenever what a snapshot key promises changes, so a snapshot written
/// by an older engine is ignored rather than read as the answer to a question it
/// was never asked. Version 2 is the per-package ecosystem: before it every
/// package in every lockfile was queried as npm, so a v1 key over a
/// `requirements.txt` stands for npm answers about PyPI names.
const KEY_VERSION: &str = "2";

/// Every lockfile at the root of `root`, in [`CANDIDATES`] order, and empty when
/// the repository has none.
///
/// A lockfile that exists but cannot be read, because a permission denies it or
/// because it is not UTF-8, is skipped with a warning rather than an error: the
/// module doc's rule is that a problem with this input never fails the run, and
/// a file the parser could not have made sense of anyway is exactly that. The
/// other lockfiles beside it are still read.
pub fn read(root: &Path) -> anyhow::Result<Vec<Lockfile>> {
    let mut out = Vec::new();
    for (name, ecosystem, parse) in CANDIDATES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}; dependency advisories skipped", path.display());
                continue;
            }
        };
        let packages = normalize(parse(&text));
        out.push(Lockfile { rel: name.to_string(), hash: package_hash(ecosystem, &packages), packages });
    }
    Ok(out)
}

/// The repo-relative paths of the lockfiles [`read`] would parse, without
/// reading or parsing them, and empty when the repository has none.
///
/// A run narrowed to a scope has to know whether a lockfile is in that scope
/// before it decides whether to run the rule that reads it, and that question
/// must not cost the read the answer may make unnecessary: a `package-lock.json`
/// is routinely a megabyte or more, and the whole point of the check is that a
/// scope which cannot report an advisory never opens it.
pub fn locate(root: &Path) -> Vec<&'static str> {
    CANDIDATES.iter().map(|(name, _, _)| *name).filter(|name| root.join(name).is_file()).collect()
}

/// The key a snapshot of these packages is stored under: a version marker, the
/// lockfile's ecosystem, then `ecosystem:name@version` per line, sorted, hashed.
/// See [`Lockfile::hash`] and [`KEY_VERSION`].
///
/// The ecosystem is named twice on purpose. Each line carries it because that is
/// what a package is; the seed carries it because an empty `composer.lock` and
/// an empty `requirements.txt` are still two different questions.
fn package_hash(ecosystem: &str, packages: &[Package]) -> String {
    let mut lines: Vec<String> = packages.iter().map(|p| format!("{}:{}@{}", p.ecosystem, p.name, p.version)).collect();
    // `normalize` already sorted by name then version then line, which orders
    // these the same way, but the hash does not depend on a caller having done
    // that: the point of the key is that one list has one spelling.
    lines.sort();
    content_hash(&format!("locrin-lockfile-v{KEY_VERSION}\n{ecosystem}\n{}", lines.join("\n")))
}

/// Deduplicates by (ecosystem, name, version), keeping the earliest line, and
/// sorts.
///
/// A lockfile lists the same version of a transitive dependency once per place
/// it is hoisted to; one advisory finding per version is what a reader wants,
/// and the earliest line is the one nearest the top of the file.
fn normalize(mut packages: Vec<Package>) -> Vec<Package> {
    packages.sort();
    packages.dedup_by(|a, b| a.name == b.name && a.version == b.version && a.ecosystem == b.ecosystem);
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
            ecosystem: NPM,
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
        out.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            line: idx as u32 + 1,
            ecosystem: NPM,
        });
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
        out.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            line: idx as u32 + 1,
            ecosystem: NPM,
        });
    }
    out
}

/// `composer.lock`: the `packages` and `packages-dev` arrays.
///
/// Both are installed. `packages-dev` is what `composer install` puts on disk
/// outside production, which is a test runner and a static analyser running in
/// CI with the repository checked out, so an advisory against one is an advisory
/// against this repository.
///
/// Composer records a tagged release as the tag, and the tag conventionally
/// carries a `v` the registry's version does not (`v1.1.3` is release `1.1.3`).
/// One leading `v` is stripped, because the advisory database is asked about the
/// version and not about the tag.
fn parse_composer(text: &str) -> Vec<Package> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let lines = value_lines(text, "name");
    let mut out = Vec::new();
    for section in ["packages", "packages-dev"] {
        let entries = root.get(section).and_then(|p| p.as_array());
        for entry in entries.into_iter().flatten() {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else { continue };
            let Some(version) = entry.get("version").and_then(|v| v.as_str()) else { continue };
            let version = version.strip_prefix('v').unwrap_or(version);
            if name.is_empty() || version.is_empty() {
                continue;
            }
            // `dev-main` and `1.x-dev` are branch aliases: whatever the branch
            // pointed at on the day the install ran, which is not a release and
            // not a version osv.dev has an answer about. Asking costs a slot in
            // the batch and returns nothing, so the entry is left out.
            if version.starts_with("dev-") || version.ends_with("-dev") {
                continue;
            }
            out.push(Package {
                name: name.to_string(),
                version: version.to_string(),
                line: lines.get(name).copied().unwrap_or(1),
                ecosystem: PACKAGIST,
            });
        }
    }
    out
}

/// `poetry.lock`: the `[[package]]` tables' own `name` and `version`.
///
/// Read as text rather than as TOML. The file is regular to the point of being
/// generated, the two keys wanted are scalars at the top of each table, and a
/// TOML dependency would be carried by the whole engine for one parser. The
/// table's own keys are what is read: a `[package.source]` sub-table carries a
/// `reference` whose value is frequently the word `name`, and everything after
/// the first `[` belongs to a sub-table rather than to the package.
fn parse_poetry(text: &str) -> Vec<Package> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.trim() != "[[package]]" {
            continue;
        }
        let mut name: Option<(&str, u32)> = None;
        let mut version: Option<&str> = None;
        for (off, body) in lines[idx + 1..].iter().enumerate() {
            let body = body.trim();
            if body.starts_with('[') {
                break;
            }
            if name.is_none() {
                name = toml_string(body, "name").map(|v| (v, (idx + off) as u32 + 2));
            }
            if version.is_none() {
                version = toml_string(body, "version");
            }
            if name.is_some() && version.is_some() {
                break;
            }
        }
        let (Some((name, at)), Some(version)) = (name, version) else { continue };
        if name.is_empty() || version.is_empty() {
            continue;
        }
        out.push(Package { name: pypi_name(name), version: version.to_string(), line: at, ecosystem: PYPI });
    }
    out
}

/// `requirements.txt`: the `name==version` lines and nothing else.
///
/// The file is a list of install arguments rather than a lockfile, so only the
/// lines that pin one exact version say what an install puts on disk. A `>=` or
/// a `~=` is a range whose answer depends on the day the install ran, `-r` and
/// `-e` point somewhere else entirely, and an advisory query needs a version.
/// Those lines are skipped rather than guessed at: a wrong version in a security
/// finding is worse than a missing one.
fn parse_requirements(text: &str) -> Vec<Package> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        // A trailing comment, then an environment marker: `pytest==6.2.2 ;
        // python_version >= "3.6"` is still a pinned install of pytest.
        let line = line.split('#').next().unwrap_or_default();
        let line = line.split(';').next().unwrap_or_default().trim();
        // `-r other.txt`, `-e .`, `--index-url ...`: an instruction, not a package.
        if line.is_empty() || line.starts_with('-') {
            continue;
        }
        let Some((name, version)) = line.split_once("==") else { continue };
        // A hash or an option can follow the version on the same line.
        let version = version.split_whitespace().next().unwrap_or_default();
        // `1.0,<2` is two specifiers and `1.*` is a range: neither is a version.
        if version.is_empty() || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
            continue;
        }
        // `requests[security]` is how the package is installed, not what it is
        // called, and the registry knows it by the bare name.
        let name = name.split('[').next().unwrap_or_default().trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            continue;
        }
        out.push(Package {
            name: pypi_name(name),
            version: version.to_string(),
            line: idx as u32 + 1,
            ecosystem: PYPI,
        });
    }
    out
}

/// A PyPI name as the registry spells it: `Flask_SQLAlchemy` and
/// `flask-sqlalchemy` are one project, and osv.dev is asked about the
/// normalised form (PEP 503).
fn pypi_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

/// `key = "value"` as its value, for the flat scalar keys of a TOML table.
fn toml_string<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Splits `<name>@<rest>` at the last `@` that is not at index zero, so a scoped
/// name keeps the `@` it starts with.
fn split_name(spec: &str) -> Option<(&str, &str)> {
    let at = spec.rfind('@').filter(|&at| at > 0)?;
    Some((&spec[..at], &spec[at + 1..]))
}

/// The first line each `"<key>": "<value>"` pair appears on, keyed by the value.
///
/// `package-lock.json` names a package in its key and `composer.lock` names it
/// in a value, so the two need opposite sides of the same lookup. Both exist
/// because `serde_json` records no positions and a finding points at a line.
fn value_lines<'a>(text: &'a str, key: &str) -> HashMap<&'a str, u32> {
    let needle = format!("\"{key}\"");
    let mut lines = HashMap::new();
    for (idx, line) in text.lines().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix(needle.as_str()) else { continue };
        let Some(rest) = rest.trim_start().strip_prefix(':') else { continue };
        let Some(rest) = rest.trim_start().strip_prefix('"') else { continue };
        let Some(end) = rest.find('"') else { continue };
        lines.entry(&rest[..end]).or_insert(idx as u32 + 1);
    }
    lines
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

    /// The package list every npm-family fixture describes, with the line each
    /// entry sits on.
    fn expected(lines: [u32; 4]) -> Vec<Package> {
        let names = [("@acme/util", "2.0.0"), ("b", "3.1.0"), ("left-pad", "1.3.0"), ("lodash", "4.17.15")];
        names
            .iter()
            .zip(lines)
            .map(|((name, version), line)| Package {
                name: name.to_string(),
                version: version.to_string(),
                line,
                ecosystem: NPM,
            })
            .collect()
    }

    fn php(name: &str, version: &str, line: u32) -> Package {
        Package { name: name.to_string(), version: version.to_string(), line, ecosystem: PACKAGIST }
    }

    fn pypi(name: &str, version: &str, line: u32) -> Package {
        Package { name: name.to_string(), version: version.to_string(), line, ecosystem: PYPI }
    }

    /// The lockfile [`read`] answers with first, which is the npm one in the
    /// fixture directory.
    fn first(root: &std::path::Path) -> Lockfile {
        read(root).unwrap().into_iter().next().expect("the fixture directory has lockfiles")
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
        let lock = first(&fixtures());
        assert_eq!(lock.rel, "package-lock.json");
        assert_eq!(lock.hash, package_hash(NPM, &expected([16, 20, 24, 32])));
        assert_ne!(lock.hash, content_hash(&text("package-lock.json")), "the text is not what is hashed");
        assert_eq!(lock.packages, expected([16, 20, 24, 32]));
    }

    /// The hash keys a snapshot that is zipped positionally against the package
    /// list, so it has to answer for the list and not for the text that produced
    /// it: three lockfile formats describing one install share it, and a fourth
    /// package changes it.
    #[test]
    fn the_hash_follows_the_package_list_and_not_the_text() {
        let npm = package_hash(NPM, &normalize(parse_npm(&text("package-lock.json"))));
        let yarn = package_hash(NPM, &normalize(parse_yarn(&text("yarn.lock"))));
        let pnpm = package_hash(NPM, &normalize(parse_pnpm(&text("pnpm-lock.yaml"))));
        assert_eq!(npm, yarn, "the same packages at different lines hash the same");
        assert_eq!(npm, pnpm);

        let mut more = normalize(parse_npm(&text("package-lock.json")));
        more.push(Package { name: "extra".into(), version: "1.0.0".into(), line: 99, ecosystem: NPM });
        assert_ne!(npm, package_hash(NPM, &normalize(more)), "one more package is a different list");
    }

    /// `locate` and `read` have to name the same file: a scoped run decides
    /// whether to run the advisory rule from `locate` and the rule then reports
    /// against `read`, so a disagreement would run the rule and discard its
    /// findings, or skip it over a lockfile that was in scope.
    #[test]
    fn locate_names_the_files_read_would_parse() {
        let located: Vec<String> = locate(&fixtures()).into_iter().map(str::to_string).collect();
        assert_eq!(located.first().map(String::as_str), Some("package-lock.json"));
        assert_eq!(located, read(&fixtures()).unwrap().into_iter().map(|l| l.rel).collect::<Vec<_>>());
    }

    #[test]
    fn read_finds_nothing_in_a_directory_without_a_lockfile() {
        let dir = std::env::temp_dir().join(format!("locrin-lockfile-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir).unwrap().is_empty());
        assert!(locate(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lockfile that is not text is an unreadable input, not a broken run.
    #[test]
    fn a_lockfile_that_is_not_utf8_reads_as_no_lockfile_rather_than_an_error() {
        let dir = std::env::temp_dir().join(format!("locrin-lockfile-binary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package-lock.json"), [0x7b, 0xff, 0xfe, 0x7d]).unwrap();
        std::fs::write(dir.join("requirements.txt"), "urllib3==1.26.4\n").unwrap();
        let locks = read(&dir).unwrap();
        assert_eq!(
            locks.iter().map(|l| l.rel.as_str()).collect::<Vec<_>>(),
            vec!["requirements.txt"],
            "the unreadable lockfile is skipped, and the readable one beside it is still checked"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lockfile_that_does_not_parse_yields_no_packages_rather_than_an_error() {
        assert!(parse_npm("{ not json").is_empty());
        assert!(parse_yarn("").is_empty());
        assert!(parse_pnpm("packages:\n").is_empty());
        assert!(parse_composer("{ not json").is_empty());
        assert!(parse_composer(r#"{"packages": "not an array"}"#).is_empty());
        assert!(parse_poetry("[[package]]\nname = \"no-version\"\n").is_empty());
        assert!(parse_requirements("# nothing pinned here\n").is_empty());
    }

    #[test]
    fn composer_reads_both_sections_and_strips_one_leading_v_from_the_version() {
        let packages = normalize(parse_composer(&text("composer.lock")));
        assert_eq!(
            packages,
            vec![php("monolog/monolog", "2.0.0", 8), php("phpunit/phpunit", "9.5.0", 37), php("psr/log", "1.1.3", 27),],
            "packages-dev is installed too, and `v1.1.3` is the release `1.1.3`"
        );
    }

    /// A branch alias is a moving target rather than a release, and osv.dev has
    /// no answer about one. Querying it costs a slot in the batch and returns
    /// nothing, so the entry is left out of the list entirely.
    #[test]
    fn composer_skips_a_branch_alias_rather_than_asking_about_it() {
        let packages = parse_composer(
            r#"{"packages": [
                 {"name": "a/one", "version": "dev-main"},
                 {"name": "b/two", "version": "1.x-dev"},
                 {"name": "c/three", "version": "v2.9.0-dev"},
                 {"name": "d/four", "version": "v1.2.3"}
               ]}"#,
        );
        assert_eq!(
            packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["d/four"],
            "only the tagged release is a version the advisory database can answer about"
        );
    }

    #[test]
    fn poetry_reads_the_package_tables_and_normalises_the_name() {
        let packages = normalize(parse_poetry(&text("poetry.lock")));
        assert_eq!(
            packages,
            vec![
                pypi("certifi", "2019.11.28", 4),
                pypi("requests", "2.19.0", 11),
                // PyPI knows `Zope_Interface` as `zope-interface`, and an
                // advisory query has to spell it the way the registry does.
                pypi("zope-interface", "4.7.1", 27),
            ]
        );
    }

    /// A `[package.source]` table carries `reference = "name"`, which is the
    /// shape that would turn a sub-table into a package if the parser read past
    /// the table's own keys.
    #[test]
    fn poetry_does_not_read_a_sub_table_as_a_package() {
        assert_eq!(normalize(parse_poetry(&text("poetry.lock"))).len(), 3);
        assert!(!parse_poetry(&text("poetry.lock")).iter().any(|p| p.name == "name"));
    }

    /// Only a pinned `==` line says which version is installed. Everything else
    /// in the file is a constraint, an include or a comment, and a range is not
    /// a version an advisory query can answer for.
    #[test]
    fn requirements_reads_pinned_lines_only() {
        let packages = normalize(parse_requirements(&text("requirements.txt")));
        assert_eq!(
            packages,
            vec![
                pypi("flask-sqlalchemy", "2.4.1", 7),
                pypi("numpy", "1.21.0", 10),
                pypi("pytest", "6.2.2", 11),
                // The extra is how the package is installed, not what it is called.
                pypi("requests", "2.19.0", 6),
                pypi("urllib3", "1.26.4", 5),
            ],
            ">= and < lines, -r, -e, comments and a bare name are all skipped"
        );
    }

    #[test]
    fn read_returns_every_lockfile_the_repository_has_with_its_own_ecosystem() {
        let locks = read(&fixtures()).unwrap();
        let seen: Vec<(&str, &str)> =
            locks.iter().map(|l| (l.rel.as_str(), l.packages.first().map(|p| p.ecosystem).unwrap_or(""))).collect();
        assert_eq!(
            seen,
            vec![
                ("package-lock.json", NPM),
                ("yarn.lock", NPM),
                ("pnpm-lock.yaml", NPM),
                ("composer.lock", PACKAGIST),
                ("poetry.lock", PYPI),
                ("requirements.txt", PYPI),
            ],
            "a repository with a Composer lockfile beside an npm one is checked for both"
        );
        assert_eq!(locate(&fixtures()), locks.iter().map(|l| l.rel.as_str()).collect::<Vec<_>>());
    }

    /// [`CANDIDATES`] names each parser's ecosystem beside it, and the parser
    /// stamps it on every package it produces. The two are written down twice,
    /// so the run has to say they agree: a snapshot keyed for PyPI holding
    /// packages queried as npm is the one way this module can lie.
    #[test]
    fn every_parser_agrees_with_the_ecosystem_beside_it() {
        for (name, ecosystem, parse) in CANDIDATES {
            let packages = parse(&text(name));
            assert!(!packages.is_empty(), "{name} has to parse for this to mean anything");
            assert!(packages.iter().all(|p| p.ecosystem == ecosystem), "{name} does not stamp {ecosystem}");
        }
    }

    /// The key the advisory snapshot is stored under has to answer for the
    /// ecosystem as well as for the package list: `requests` is an npm package
    /// and a PyPI package with unrelated advisories, and a snapshot fetched for
    /// one must never be served as the answer for the other.
    #[test]
    fn the_key_separates_two_ecosystems_holding_the_same_name_and_version() {
        let npm = vec![Package { name: "requests".into(), version: "2.19.0".into(), line: 1, ecosystem: NPM }];
        let pypi = vec![Package { name: "requests".into(), version: "2.19.0".into(), line: 1, ecosystem: PYPI }];
        assert_ne!(package_hash(NPM, &npm), package_hash(PYPI, &pypi));
        // And an empty list is still two different questions.
        assert_ne!(package_hash(NPM, &[]), package_hash(PYPI, &[]));
    }

    #[test]
    fn duplicate_versions_collapse_to_the_earliest_line() {
        let dupes = vec![
            Package { name: "lodash".into(), version: "4.17.15".into(), line: 40, ecosystem: NPM },
            Package { name: "lodash".into(), version: "4.17.15".into(), line: 12, ecosystem: NPM },
        ];
        assert_eq!(
            normalize(dupes),
            vec![Package { name: "lodash".into(), version: "4.17.15".into(), line: 12, ecosystem: NPM }]
        );
    }
}
