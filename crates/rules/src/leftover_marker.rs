//! Flags TODO / FIXME / HACK / XXX comments that carry no issue reference.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{clean_files, finding, Language, Rule, RuleContext, Scope, ALL};

pub struct LeftoverMarker;

const MARKERS: &[&str] = &["TODO", "FIXME", "HACK", "XXX"];

fn has_marker(text: &str) -> bool {
    MARKERS.iter().any(|m| {
        text.match_indices(m).any(|(i, _)| {
            let before = text[..i].chars().last().map(|c| !c.is_alphanumeric()).unwrap_or(true);
            let after = text[i + m.len()..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true);
            before && after
        })
    })
}

fn has_reference(text: &str) -> bool {
    if text.contains("http") {
        return true;
    }
    let bytes = text.as_bytes();
    // #123
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && bytes.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return true;
        }
    }
    // ABC-123
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_uppercase() {
            i += 1;
        }
        let digit_after_dash = bytes.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false);
        if i - start >= 2 && i < bytes.len() && bytes[i] == b'-' && digit_after_dash {
            return true;
        }
        i += 1;
    }
    false
}

fn comment_lines(node: Node, src: &str, out: &mut Vec<(u32, String)>) {
    if node.kind() == "comment" {
        let text = node.utf8_text(src.as_bytes()).unwrap_or("");
        let base = node.start_position().row as u32 + 1;
        for (i, l) in text.lines().enumerate() {
            out.push((base + i as u32, l.to_string()));
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        comment_lines(child, src, out);
    }
}

impl Rule for LeftoverMarker {
    fn id(&self) -> &'static str {
        "leftover-agent-marker"
    }
    fn description(&self) -> &'static str {
        "TODO or FIXME without an issue reference"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }
    /// Every language: what this rule reads is a comment node and the word
    /// inside it, and every grammar the engine loads calls a comment a
    /// `comment`. PHP's covers `//`, `#` and `/* */`; Python's covers `#`.
    fn languages(&self) -> &'static [Language] {
        ALL
    }
    /// Off for PHP: the precision gate's second round
    /// (`docs/superpowers/plans/2026-09-11-php-and-python-precision.md`, round
    /// two, Monica v4.1.2) scored 4 true of 5, which is 80 percent of a sample
    /// of exactly five and under the 85 the gate asks for. The false finding
    /// was `XXX` inside the placeholder path `avatars/XXX.jpg` in a prose
    /// comment. Pooled with round one's 2 of 2 on BookStack the pair is 6 of
    /// 7, and the fix is one clause in `has_marker` (a marker is not a marker
    /// inside backticks or a path); the pair comes back on when that lands
    /// and the pair is re-measured.
    fn enabled_for(&self, lang: Language) -> bool {
        lang != Language::Php
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            let mut lines = Vec::new();
            comment_lines(file.tree.root_node(), &file.source, &mut lines);
            for (line, text) in lines {
                if has_marker(&text) && !has_reference(&text) {
                    out.push(finding(self, file, line, text.trim(), "Link the marker to an issue or resolve it now"));
                }
            }
        }
        Ok(out)
    }
}
