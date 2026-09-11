//! Flags TODO / FIXME / HACK / XXX comments that carry no issue reference.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{clean_files, finding, Language, Rule, RuleContext, Scope, ALL};

pub struct LeftoverMarker;

const MARKERS: &[&str] = &["TODO", "FIXME", "HACK", "XXX"];

/// The characters a path or a file name is spelled with. A marker word glued
/// to one of them on either side is part of a name, not a note to a reader:
/// `avatars/XXX.jpg`, `XXX-XXX-XXXX`, `fixtures/TODO_list.json`.
///
/// `/` is the one that is not a path character on its own, because it is also
/// how three of the four languages open a comment and how a writer separates
/// two markers. See [`preceded_by_path`] and [`followed_by_path`].
const PATH_CHARS: &[char] = &['/', '.', '_', '-'];

/// Whether what sits before a marker word puts it inside a path.
///
/// A `/` counts only when a name character sits before it: `avatars/TODO` is a
/// path segment, and `//TODO` is the comment's own opener with the marker
/// glued to it. The comment text a rule reads still carries that opener, so
/// without the distinction every unspaced `//TODO:` in TypeScript, JavaScript
/// and PHP was read as a file name and reported nothing. Python is unaffected:
/// its comments open with `#`, which is not a path character.
fn preceded_by_path(before: &str) -> bool {
    let mut back = before.chars().rev();
    let Some(c) = back.next() else { return false };
    if !PATH_CHARS.contains(&c) {
        return false;
    }
    c != '/' || back.next().is_some_and(|p| p.is_alphanumeric())
}

/// Whether what sits after a marker word puts it inside a path. A `.` is left
/// to [`opens_extension`], which is the narrower question of a file extension.
///
/// A `/` counts only when what follows it is a name: `XXX/thumb.jpg` is a path,
/// while `TODO/FIXME both` is one note written with two marker words and
/// `TODO/ handle` is prose. So a `/` followed by another marker word or by a
/// space is not a separator.
fn followed_by_path(after: &str) -> bool {
    let Some(c) = after.chars().next() else { return false };
    if c == '.' || !PATH_CHARS.contains(&c) {
        return false;
    }
    if c == '/' {
        let rest = &after[1..];
        return !rest.starts_with(' ') && !MARKERS.iter().any(|m| rest.starts_with(m));
    }
    true
}

/// Whether the text after a marker word opens a file extension: a `.` and one
/// to five letters or digits, as in `XXX.jpg` or `TODO.md`. A `.` closing a
/// sentence (`TODO.` at the end of a line, or before a space) is not one.
fn opens_extension(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('.') else { return false };
    let run = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).count();
    (1..=5).contains(&run) && rest.chars().nth(run).is_none_or(|c| !c.is_alphanumeric())
}

/// Whether the text holds one of the marker words on its own: not inside a
/// longer word, and not inside a path or a file name.
fn has_marker(text: &str) -> bool {
    MARKERS.iter().any(|m| {
        text.match_indices(m).any(|(i, _)| {
            let before = text[..i].chars().last();
            let after = &text[i + m.len()..];
            let next = after.chars().next();
            let word = before.is_none_or(|c| !c.is_alphanumeric()) && next.is_none_or(|c| !c.is_alphanumeric());
            let in_path = preceded_by_path(&text[..i]) || followed_by_path(after) || opens_extension(after);
            word && !in_path
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
    /// On for PHP again. The precision gate's second round
    /// (`docs/superpowers/plans/2026-09-11-php-and-python-precision.md`, round
    /// two, Monica v4.1.2) scored 4 true of 5 and turned the pair off; the one
    /// false finding was `XXX` inside the placeholder path `avatars/XXX.jpg`.
    /// Round three made a marker word glued to a path character (`/`, `.`,
    /// `_`, `-`) or opening a file extension no marker at all, and the
    /// re-measure on the same corpus with the same seed scored every remaining
    /// finding true (round three of the same document), so the default is the
    /// trait's: on everywhere.
    fn enabled_for(&self, _lang: Language) -> bool {
        true
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
