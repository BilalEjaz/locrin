//! Flags runs of commented-out code. Heuristic and advisory by design.
//!
//! The heuristic is deliberately asymmetric. A comma or a closing parenthesis
//! at the end of a line is common in ordinary prose, so those count only as
//! supporting signals: a run is reported only when at least one line carries a
//! strong signal (a `;`, `{` or `}` ending, or a statement keyword opening with
//! code on the same line) and at least half of the run's lines look like code
//! at all. Trailing comments are never part of a run, because a column of unit
//! annotations beside real code is not a commented-out block.
//!
//! Three tests keep prose out, each of them paid for by a false positive on the
//! public benchmark. A statement keyword is an ordinary word before it is a
//! keyword, so it opens a statement only with the shape of one on its line: an
//! assignment, a call, or a code ending. `let x = 1` is code and `let the table
//! carry the advice` is a sentence about one. A line that ends a sentence, a
//! full stop or a question or exclamation mark behind four or more words, is
//! prose whatever it opens or closes with; that is what a paragraph ending
//! `(NeedsFocus).` or `... which is this connection's own host.` is. And a
//! block is code only when most of it is: one sentence over a disabled
//! function is still a disabled function, while one `$` line inside five lines
//! of French commentary is a sentence that happens to name a variable.
//!
//! The run detection is the same in every language; only the vocabulary the run
//! is tested against changes, because what a statement looks like is what the
//! language says it looks like. A Python statement ends in a colon or in
//! nothing at all, so a semicolon is no use there and `return `, `import ` and
//! the block headers do the work instead. A PHP statement ends in a semicolon
//! like a JavaScript one, and opens with a sigil, a visibility keyword or
//! `foreach (`.
//!
//! Python's keywords are also ordinary English words, which JavaScript's
//! punctuation-carrying `if (` and `for (` are not, so a block header counts
//! only with the colon that closes it and `from ` only with the ` import ` that
//! follows it. Without that, three sentences of prose opening "for now..." and
//! "if the input..." read as a commented-out loop. The colon cuts the other
//! way too: a trailing colon alone is a prose header (`# Note:`, `# Returns:`)
//! as often as it is a block, so it is strong only behind a suite keyword.

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
    /// Openings that begin a statement only when the rest of the line agrees.
    /// Python's keywords are ordinary English words, so the prefix alone says
    /// nothing: see [`Needs`].
    qualified_starts: &'static [(&'static str, Needs)],
}

/// What the rest of a line has to carry before a qualified opening counts as a
/// statement.
#[derive(Clone, Copy)]
enum Needs {
    /// The colon that opens the block. `for row in rows:` is code and `for now
    /// this is fine.` is a sentence, and the colon is the whole difference; the
    /// same holds for every Python keyword that opens a suite.
    BlockColon,
    /// An ` import ` later in the line. `from ` opens an import and opens about
    /// as many sentences of prose.
    Import,
}

impl Needs {
    fn met(self, trimmed: &str) -> bool {
        match self {
            Needs::BlockColon => trimmed.ends_with(':'),
            Needs::Import => trimmed.contains(" import "),
        }
    }
}

const JS: Vocabulary = Vocabulary {
    strong_endings: &[';', '{', '}'],
    endings: &[';', '{', '}', ')', ','],
    starts: &["const ", "let ", "var ", "return ", "if (", "for (", "import ", "export "],
    // A JavaScript keyword carries its own punctuation: `if (` and `for (` are
    // already the shapes prose does not write.
    qualified_starts: &[],
};

const PHP: Vocabulary = Vocabulary {
    strong_endings: &[';', '{', '}'],
    endings: &[';', '{', '}', ')', ','],
    // `$` on its own: every PHP variable carries the sigil, so a commented-out
    // assignment opens with it where a sentence of prose does not.
    starts: &["$", "function ", "return ", "if (", "foreach (", "echo ", "use ", "namespace ", "public ", "private "],
    qualified_starts: &[],
};

const PY: Vocabulary = Vocabulary {
    // No ending is strong on its own. Python has no statement terminator, and
    // the colon that opens every block is also what a prose header ends on:
    // `# Note:`, `# Args:`, `# Options Used:`. A colon is therefore strong only
    // behind a suite keyword, which is what the `BlockColon` starts below say,
    // and on its own it is a supporting signal like a comma.
    strong_endings: &[],
    endings: &[':', ')', ','],
    // The bare ones are the openings that are not English: `return ` and
    // `import ` head a sentence far more rarely than `if` or `for` do, `self.`
    // is a name, and `print(` carries its own parenthesis.
    starts: &["return ", "import ", "self.", "print("],
    // Everything that opens a Python suite is a word a comment is written with,
    // so each is read only with the colon that closes its header. `#  for now
    // this handles the simple case.` is prose, and nothing about its first two
    // characters says otherwise.
    qualified_starts: &[
        ("if ", Needs::BlockColon),
        ("elif ", Needs::BlockColon),
        ("else", Needs::BlockColon),
        ("for ", Needs::BlockColon),
        ("while ", Needs::BlockColon),
        ("with ", Needs::BlockColon),
        ("try", Needs::BlockColon),
        ("except", Needs::BlockColon),
        ("class ", Needs::BlockColon),
        ("def ", Needs::BlockColon),
        ("from ", Needs::Import),
    ],
};

fn vocabulary(language: Language) -> &'static Vocabulary {
    match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => &JS,
        Language::Php => &PHP,
        Language::Python => &PY,
    }
}

/// Whether a trimmed line opens with something that can only open a statement.
/// The opening alone is never the verdict: [`opens_a_statement`] is what both
/// predicates ask, and it is one test for both of them, because an opening too
/// weak to qualify a run is too weak to support one either, or a page of prose
/// about a `for` loop would be two supporting signals away from a finding.
fn starts_a_statement(trimmed: &str, v: &Vocabulary) -> bool {
    v.starts.iter().any(|s| trimmed.starts_with(s)) || starts_a_qualified_statement(trimmed, v)
}

/// The half of [`starts_a_statement`] that already reads the rest of the line.
/// A qualified opening carries the shape of a statement in what qualifies it
/// (the colon that closes a Python block header, the ` import ` behind
/// `from `), so [`opens_a_statement`] asks nothing further of it.
fn starts_a_qualified_statement(trimmed: &str, v: &Vocabulary) -> bool {
    v.qualified_starts.iter().any(|(prefix, needs)| trimmed.starts_with(prefix) && needs.met(trimmed))
}

/// An opening plus the shape of a statement on the same line.
///
/// Every bare opening in the vocabulary is also an ordinary word or sigil:
/// `let SUGGESTS carry the advice`, `return to the records first`, `$config is
/// this connection's own host`. The prefix alone therefore says nothing, and
/// the line has to carry code as well: an assignment, a call, a PHP arrow, or
/// one of the endings a statement closes with.
fn opens_a_statement(trimmed: &str, v: &Vocabulary) -> bool {
    starts_a_statement(trimmed, v) && (starts_a_qualified_statement(trimmed, v) || has_code_shape(trimmed, v))
}

/// Code somewhere on the line rather than at its start: an assignment, a call,
/// a PHP member access, or an ending only a statement produces. `==` is not an
/// assignment, but every other `=` shape (`=`, `+=`, `=>`) is close enough to
/// code that prose does not write it.
fn has_code_shape(trimmed: &str, v: &Vocabulary) -> bool {
    trimmed.contains('(')
        || trimmed.contains("->")
        || trimmed.ends_with(v.endings)
        || trimmed
            .match_indices('=')
            .any(|(i, _)| trimmed.as_bytes().get(i + 1) != Some(&b'=') && (i == 0 || trimmed.as_bytes()[i - 1] != b'='))
}

/// A finished sentence: four or more words closing on a full stop, a question
/// mark or an exclamation mark, with a trailing quote or bracket stripped
/// first so `(NeedsFocus).` and `carry the advice (it already holds it).` read
/// as the sentence endings they are. The word count is what keeps `print("done
/// .")` and other one-token lines out of it.
fn is_sentence(trimmed: &str) -> bool {
    let t = trimmed.trim_end_matches([')', ']', '"', '\'', '`']);
    t.ends_with(['.', '?', '!']) && trimmed.split_whitespace().count() >= 4
}

/// A line carrying at least a supporting signal of being code, and not a
/// sentence: a sentence is prose whatever it opens or closes with.
fn looks_like_code(line: &str, language: Language) -> bool {
    let v = vocabulary(language);
    let t = line.trim();
    if t.is_empty() || is_sentence(t) {
        return false;
    }
    t.ends_with(v.endings) || opens_a_statement(t, v)
}

/// A signal prose rarely produces: a statement terminator, a brace, a colon
/// opening a block, or a keyword opening a statement it goes on to write.
fn is_strong_code(line: &str, language: Language) -> bool {
    let v = vocabulary(language);
    let t = line.trim();
    if t.is_empty() || is_sentence(t) {
        return false;
    }
    t.ends_with(v.strong_endings) || opens_a_statement(t, v)
}

/// The qualifying test for a run: one strong signal at minimum, and most of the
/// run looking like code. "Most" is half, so a single sentence explaining a
/// disabled function leaves the block a finding, while a block that is mostly
/// sentences is prose however much of a statement one of its lines opens with.
fn is_commented_code(lines: &[&str], language: Language) -> bool {
    let body: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    body.iter().any(|l| is_strong_code(l, language))
        && body.iter().filter(|l| looks_like_code(l, language)).count() * 2 >= body.len()
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
    /// Off for Python: the precision gate's second round
    /// (`docs/superpowers/plans/2026-09-11-php-and-python-precision.md`, round
    /// two, Poetry 2.4.3) scored 0 true of 14. Every finding was a prose
    /// comment whose header line ends in a colon (`# Options Used:`,
    /// `# For instance:`), which was the one ending the Python vocabulary
    /// treated as strong. Round three made a colon strong only behind a suite
    /// keyword, and the re-run (round three of the same document) left 1 of
    /// the 14 on Poetry and 0 on FastSpot: `# with the following overrides:`
    /// opens with `with ` and ends in a colon, so it reads as a `with` block
    /// header, and it is prose. One false positive of one finding is still
    /// a fail, so the pair stays off; the vocabulary and its fixture tests
    /// still run it directly, and it comes back on once a `with` header
    /// needs the shape of one (a `(`, an ` as `, or a dotted name) and the
    /// pair is re-measured.
    fn enabled_for(&self, lang: Language) -> bool {
        lang != Language::Python
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
