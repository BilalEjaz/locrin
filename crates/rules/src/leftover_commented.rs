//! Flags runs of commented-out code. Heuristic and advisory by design.
//!
//! The heuristic is deliberately asymmetric. A comma or a closing parenthesis
//! at the end of a line is common in ordinary prose, so those count only as
//! supporting signals: a run is reported only when at least one line carries a
//! strong signal (a `;`, `{` or `}` ending, or a statement keyword opening) and
//! at least two lines look like code at all. Trailing comments are never part
//! of a run, because a column of unit annotations beside real code is not a
//! commented-out block.
//!
//! The run detection is the same in every language; only the vocabulary the run
//! is tested against changes, because what a statement looks like is what the
//! language says it looks like. A Python statement ends in a colon or in
//! nothing at all, so a semicolon is no use there and `def `, `class ` and
//! `return ` do the work instead. A PHP statement ends in a semicolon like a
//! JavaScript one, and opens with a sigil, a visibility keyword or `foreach (`.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use tree_sitter::Node;

use crate::{clean_files, finding, Language, Rule, RuleContext, Scope, ALL};

pub struct LeftoverCommented;

/// What a statement looks like in one language: the endings only code produces,
/// the endings prose produces too and that therefore only support a verdict,
/// and the openings that can only begin a statement.
struct Vocabulary {
    /// An ending prose rarely produces. On its own it qualifies a run.
    strong_endings: &'static [char],
    /// The strong endings plus the supporting ones: a comma or a closing
    /// parenthesis, which ordinary prose ends lines with all the time.
    endings: &'static [char],
    /// Openings that can only begin a statement. Each is strong on its own.
    starts: &'static [&'static str],
}

const JS: Vocabulary = Vocabulary {
    strong_endings: &[';', '{', '}'],
    endings: &[';', '{', '}', ')', ','],
    starts: &["const ", "let ", "var ", "return ", "if (", "for (", "import ", "export "],
};

const PHP: Vocabulary = Vocabulary {
    strong_endings: &[';', '{', '}'],
    endings: &[';', '{', '}', ')', ','],
    // `$` on its own: every PHP variable carries the sigil, so a commented-out
    // assignment opens with it where a sentence of prose does not.
    starts: &["$", "function ", "return ", "if (", "foreach (", "echo ", "use ", "namespace ", "public ", "private "],
};

const PY: Vocabulary = Vocabulary {
    // A colon: what opens every Python block, and what a sentence of prose
    // almost never ends on. Python has no statement terminator, so this is the
    // only ending worth anything and the openings carry the rest.
    strong_endings: &[':'],
    endings: &[':', ')', ','],
    starts: &[
        "def ", "class ", "import ", "from ", "return ", "if ", "for ", "while ", "with ", "try:", "except", "self.",
        "print(",
    ],
};

fn vocabulary(language: Language) -> &'static Vocabulary {
    match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => &JS,
        Language::Php => &PHP,
        Language::Python => &PY,
    }
}

/// A line carrying at least a supporting signal of being code.
fn looks_like_code(line: &str, language: Language) -> bool {
    let v = vocabulary(language);
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(v.endings) || v.starts.iter().any(|s| t.starts_with(s))
}

/// A signal prose rarely produces: a statement terminator, a brace, a colon
/// opening a block, or a keyword that can only open a statement.
fn is_strong_code(line: &str, language: Language) -> bool {
    let v = vocabulary(language);
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(v.strong_endings) || v.starts.iter().any(|s| t.starts_with(s))
}

/// The qualifying test for a run: one strong signal at minimum, and two lines
/// that look like code in total.
fn is_commented_code(lines: &[&str], language: Language) -> bool {
    lines.iter().any(|l| is_strong_code(l, language))
        && lines.iter().filter(|l| looks_like_code(l, language)).count() >= 2
}

fn is_license_or_doc(lines: &[&str]) -> bool {
    lines.iter().any(|l| l.contains("Copyright") || l.contains("SPDX"))
        || lines.iter().all(|l| l.trim().starts_with('*'))
}

/// The markers that open a line comment in a language. PHP writes one three
/// ways and strips `#` exactly as it strips `//`; Python writes it one way.
fn line_markers(language: Language) -> &'static [&'static str] {
    match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => &["//"],
        Language::Php => &["//", "#"],
        Language::Python => &["#"],
    }
}

fn is_line_comment(text: &str, language: Language) -> bool {
    line_markers(language).iter().any(|m| text.starts_with(m))
}

fn strip_line_comment(text: &str, language: Language) -> &str {
    let mut t = text.trim();
    for marker in line_markers(language) {
        t = t.trim_start_matches(marker);
    }
    t.trim()
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
    let language = file.language;
    let src = file.source.as_bytes();
    let mut nodes = Vec::new();
    comments(file.tree.root_node(), &mut nodes);
    nodes.sort_by_key(|n| n.start_byte());

    let mut hits: Vec<(u32, String)> = Vec::new();
    let mut run: Vec<(u32, String)> = Vec::new();
    let flush = |run: &mut Vec<(u32, String)>, hits: &mut Vec<(u32, String)>| {
        if run.len() >= 3 {
            let bodies: Vec<&str> = run.iter().map(|(_, s)| s.as_str()).collect();
            if is_commented_code(&bodies, language) && !is_license_or_doc(&bodies) {
                hits.push((run[0].0, run[0].1.clone()));
            }
        }
        run.clear();
    };

    let mut last_line: Option<u32> = None;
    for n in nodes {
        let text = n.utf8_text(src).unwrap_or("");
        let line = n.start_position().row as u32 + 1;
        if is_line_comment(text, language) && is_own_line(file, n) {
            if last_line.is_some_and(|l| l + 1 != line) {
                flush(&mut run, &mut hits);
            }
            run.push((line, strip_line_comment(text, language).to_string()));
            last_line = Some(line);
            continue;
        }
        flush(&mut run, &mut hits);
        last_line = None;
        // A trailing line comment, or a docblock. Python reaches neither block
        // branch below: its only comment is the `#` line, and a docstring is a
        // string inside an expression statement rather than a comment node, so
        // it is never scanned at all.
        if is_line_comment(text, language) || text.starts_with("/**") {
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
        if is_commented_code(&inner, language) {
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
    /// Every language: a run of commented-out statements is a comment shape,
    /// not a grammar shape. What changes per language is the vocabulary the run
    /// is tested against, which [`looks_like_code`] and [`is_strong_code`] take
    /// from the file's own language.
    fn languages(&self) -> &'static [Language] {
        ALL
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
