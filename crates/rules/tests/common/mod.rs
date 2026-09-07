use std::path::{Path, PathBuf};

use locrin_core::config::Config;
use locrin_core::finding::Finding;
use locrin_core::parse::{parse_file, ParsedFile};
use locrin_core::walk::{source_files, WalkOptions};
use locrin_rules::{run_rules, Rule, RuleContext};

pub fn fixture(rule: &str, bucket: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rule).join(bucket)
}

pub fn parse_dir(root: &Path) -> Vec<ParsedFile> {
    source_files(root, &WalkOptions::default())
        .unwrap()
        .into_iter()
        .filter_map(|p| parse_file(root, &p).unwrap())
        .collect()
}

pub fn run_on(rule: Box<dyn Rule>, root: &Path, config: &Config) -> Vec<Finding> {
    let files = parse_dir(root);
    let ctx = RuleContext { files: &files, config };
    run_rules(&[rule], &ctx)
}
