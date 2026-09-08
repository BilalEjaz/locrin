//! Flags runs of commented-out code. Heuristic and advisory by design.
//!
//! The heuristic is deliberately asymmetric. A comma or a closing parenthesis
//! at the end of a line is common in ordinary prose, so those count only as
//! supporting signals: a run is reported only when at least one line carries a
//! strong signal (a `;`, `{` or `}` ending, or a statement keyword opening) and
//! at least two lines look like code at all. Trailing comments are never part
//! of a run, because a column of unit annotations beside real code is not a
//! commented-out block.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use tree_sitter::Node;

use crate::{clean_files, finding, Rule, RuleContext, Scope};

pub struct LeftoverCommented;

const CODE_ENDINGS: &[char] = &[';', '{', '}', ')', ','];
const STRONG_ENDINGS: &[char] = &[';', '{', '}'];
const CODE_STARTS: &[&str] = &["const ", "let ", "var ", "return ", "if (", "for (", "import ", "export "];

/// A line carrying at least a supporting signal of being code.
fn looks_like_code(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(CODE_ENDINGS) || CODE_STARTS.iter().any(|s| t.starts_with(s))
}

/// A signal prose rarely produces: a statement terminator, a brace, or a
/// keyword that can only open a statement.
fn is_strong_code(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(STRONG_ENDINGS) || CODE_STARTS.iter().any(|s| t.starts_with(s))
}

/// The qualifying test for a run: one strong signal at minimum, and two lines
/// that look like code in total.
fn is_commented_code(lines: &[&str]) -> bool {
    lines.iter().any(|l| is_strong_code(l)) && lines.iter().filter(|l| looks_like_code(l)).count() >= 2
}

fn is_license_or_doc(lines: &[&str]) -> bool {
    lines.iter().any(|l| l.contains("Copyright") || l.contains("SPDX"))
        || lines.iter().all(|l| l.trim().starts_with('*'))
}

fn strip_line_comment(text: &str) -> &str {
    text.trim().trim_start_matches("//").trim()
}

/// True when nothing but whitespace precedes the comment on its own source
/// line, which is what separates a commented-out statement from an annotation
/// trailing real code.
fn is_own_line(file: &ParsedFile, node: Node) -> bool {
    let pos = node.start_position();
    let Some(line) = file.source.lines().nth(pos.row) else {
        return false;
    };
    line.as_bytes().get(..pos.column).is_some_and(|before| before.iter().all(|b| b.is_ascii_whitespace()))
}

fn comments<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.kind() == "comment" {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        comments(child, out);
    }
}

/// Every reportable run in a file, as its first line and the stripped text of
/// that line. The text is the evidence, so two blocks in one function do not
/// read identically in a report.
fn runs(file: &ParsedFile) -> Vec<(u32, String)> {
    let src = file.source.as_bytes();
    let mut nodes = Vec::new();
    comments(file.tree.root_node(), &mut nodes);
    nodes.sort_by_key(|n| n.start_byte());

    let mut hits: Vec<(u32, String)> = Vec::new();
    let mut run: Vec<(u32, String)> = Vec::new();
    let flush = |run: &mut Vec<(u32, String)>, hits: &mut Vec<(u32, String)>| {
        if run.len() >= 3 {
            let bodies: Vec<&str> = run.iter().map(|(_, s)| s.as_str()).collect();
            if is_commented_code(&bodies) && !is_license_or_doc(&bodies) {
                hits.push((run[0].0, run[0].1.clone()));
            }
        }
        run.clear();
    };

    let mut last_line: Option<u32> = None;
    for n in nodes {
        let text = n.utf8_text(src).unwrap_or("");
        let line = n.start_position().row as u32 + 1;
        if text.starts_with("//") && is_own_line(file, n) {
            if last_line.is_some_and(|l| l + 1 != line) {
                flush(&mut run, &mut hits);
            }
            run.push((line, strip_line_comment(text).to_string()));
            last_line = Some(line);
            continue;
        }
        flush(&mut run, &mut hits);
        last_line = None;
        if text.starts_with("//") || text.starts_with("/**") {
            continue;
        }
        // The licence and doc test runs on the raw lines, before the leading
        // `*` of a boxed comment is stripped, or every boxed comment would look
        // like a bare run.
        let raw: Vec<&str> = text
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if raw.len() < 3 || is_license_or_doc(&raw) {
            continue;
        }
        let inner: Vec<&str> = raw.iter().map(|l| l.trim_start_matches('*').trim()).collect();
        if is_commented_code(&inner) {
            hits.push((line, inner[0].to_string()));
        }
    }
    flush(&mut run, &mut hits);
    hits
}

impl Rule for LeftoverCommented {
    fn id(&self) -> &'static str {
        "leftover-commented-code"
    }
    fn description(&self) -> &'static str {
        "Three or more consecutive commented-out statements"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Medium
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            for (line, evidence) in runs(file) {
                out.push(finding(
                    self,
                    file,
                    line,
                    &evidence,
                    "Delete the commented-out block; version control keeps the history",
                ));
            }
        }
        Ok(out)
    }
}
