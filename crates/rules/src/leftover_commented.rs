//! Flags runs of commented-out code. Heuristic and advisory by design.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use tree_sitter::Node;

use crate::{finding, Rule, RuleContext};

pub struct LeftoverCommented;

const CODE_ENDINGS: &[char] = &[';', '{', '}', ')', ','];
const CODE_STARTS: &[&str] = &["const ", "let ", "var ", "return ", "if (", "for (", "import ", "export "];

fn looks_like_code(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(CODE_ENDINGS) || CODE_STARTS.iter().any(|s| t.starts_with(s))
}

fn is_license_or_doc(lines: &[&str]) -> bool {
    lines.iter().any(|l| l.contains("Copyright") || l.contains("SPDX"))
        || lines.iter().all(|l| l.trim().starts_with('*'))
}

fn strip_line_comment(text: &str) -> &str {
    text.trim().trim_start_matches("//").trim()
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

fn runs(file: &ParsedFile) -> Vec<u32> {
    let src = file.source.as_bytes();
    let mut nodes = Vec::new();
    comments(file.tree.root_node(), &mut nodes);
    nodes.sort_by_key(|n| n.start_byte());

    let mut hits = Vec::new();
    let mut run: Vec<(u32, String)> = Vec::new();
    let flush = |run: &mut Vec<(u32, String)>, hits: &mut Vec<u32>| {
        if run.len() >= 3 {
            let bodies: Vec<&str> = run.iter().map(|(_, s)| s.as_str()).collect();
            let code_lines = bodies.iter().filter(|l| looks_like_code(l)).count();
            if code_lines >= 2 && !is_license_or_doc(&bodies) {
                hits.push(run[0].0);
            }
        }
        run.clear();
    };

    let mut last_line: Option<u32> = None;
    for n in nodes {
        let text = n.utf8_text(src).unwrap_or("");
        let line = n.start_position().row as u32 + 1;
        if text.starts_with("//") {
            if last_line.map(|l| l + 1 != line).unwrap_or(false) {
                flush(&mut run, &mut hits);
            }
            run.push((line, strip_line_comment(text).to_string()));
            last_line = Some(line);
        } else {
            flush(&mut run, &mut hits);
            last_line = None;
            if text.starts_with("/**") {
                continue;
            }
            let inner: Vec<&str> = text
                .trim_start_matches("/*")
                .trim_end_matches("*/")
                .lines()
                .map(|l| l.trim().trim_start_matches('*').trim())
                .filter(|l| !l.is_empty())
                .collect();
            if inner.len() >= 3
                && inner.iter().filter(|l| looks_like_code(l)).count() >= 2
                && !is_license_or_doc(&inner)
            {
                hits.push(line);
            }
        }
    }
    flush(&mut run, &mut hits);
    hits
}

impl Rule for LeftoverCommented {
    fn id(&self) -> &'static str {
        "leftover-commented-code"
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

    fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
        let mut out = Vec::new();
        for file in ctx.files {
            for line in runs(file) {
                out.push(finding(
                    self,
                    file,
                    line,
                    "commented-out code block",
                    "Delete the commented-out block; version control keeps the history",
                ));
            }
        }
        out
    }
}
