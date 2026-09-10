//! Flags a value that was built rather than written reaching a sink that parses
//! its argument as HTML: React's `dangerouslySetInnerHTML`, an `innerHTML` or
//! `outerHTML` assignment, `insertAdjacentHTML`, `document.write`, and jQuery's
//! `.html()`.
//!
//! **Effectively unmeasured on the corpus.** One finding across the five
//! repositories the engine is measured against, and it was true, which is 1/1
//! and a sample far too small for the spec 10.2 gate. The rule ships on that
//! plus its fixtures; the first repository that renders markup in volume is the
//! real measurement.
//!
//! The rule reads one file's syntax tree, so it sees the shape of the value at
//! the sink and never where the value came from. That is the whole of its
//! judgement, and it is where the blind spots are:
//!
//! - **Nothing here is taint tracking.** `el.innerHTML = body` is a finding
//!   whether `body` is a request parameter or a constant built two modules away,
//!   and markup assembled in a helper and handed to the sink already sanitised
//!   reads the same as markup assembled from a query string. That is why the
//!   rule ships at Medium confidence: the shape says markup reaches a parser,
//!   not that an attacker controls it. Cross-function flow is release two's
//!   work.
//! - **A sanitiser is recognised by its name.** A call whose callee text
//!   contains `sanitize`, `purify`, `escape`, `clean`, `dompurify`, or `xss` is
//!   taken at its word, because that is the only signal a syntax tree offers:
//!   `DOMPurify.sanitize(x)`, `sanitizeHtml(x)` and `escapeHtml(x)` are the
//!   spellings application code uses. A sanitiser called `render` is invisible
//!   and a helper called `cleanUpMarkup` that sanitises nothing is believed.
//!   The names are broad on purpose: a false negative here is a reviewer's
//!   judgement call, a false positive is noise on every templating helper in the
//!   repository. The one exception is `unescape`, which contains a sanitiser
//!   name and does the opposite: `_.unescape(html)` turns entities back into
//!   tags, so a callee containing it is never exempt, however it is spelled.
//! - **A constant is not a finding, however it is spelled.** A string, a
//!   template with nothing interpolated, and an addition of those are fixed
//!   markup written here. An addition is also safe when its parts are
//!   sanitiser calls, so wrapping sanitised markup in fixed tags
//!   (`"<div>" + DOMPurify.sanitize(x) + "</div>"`) stays quiet; a template with
//!   a substitution does not get the same treatment, because the substitution
//!   may be anywhere in it and reading only the safe ones would be a guess.
//! - **`dangerouslySetInnerHTML` is read only in its documented shape.** The
//!   attribute has to carry an object literal with an `__html` key, which is how
//!   React documents it and how application code writes it.
//!   `dangerouslySetInnerHTML={props.markup}` passes an object built elsewhere
//!   and is not reported: the shape at the attribute says nothing about the
//!   value inside it.
//! - **Property names are matched, receivers are not resolved.** Any
//!   `x.innerHTML = ...` is a DOM sink here, including one on a mock or a
//!   virtual node in a test. `document.write` is the exception in the other
//!   direction: the object has to be `document`, because `stream.write` and
//!   `res.write` are not HTML sinks and are far commoner. jQuery is required to
//!   look like jQuery, `$(...)` or `jQuery(...)`, so that a `.html()` on any
//!   other object is left alone.

use std::sync::OnceLock;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use regex::Regex;
use tree_sitter::Node;

use crate::{anchor_for, clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct HtmlInjection;

const FIX: &str =
    "Render text through the framework (textContent, JSX children) or sanitise with DOMPurify before injecting";

/// A callee name that says the value has been through a sanitiser. See the
/// module doc for why a name is enough and what it costs.
const SANITIZER: &str = r"(?i)(sanitize|purify|escape|clean|dompurify|xss)";
/// The one word that contains a sanitiser name and means its opposite.
/// `unescape(x)` and `_.unescape(html)` turn `&lt;script&gt;` back into a tag,
/// which is the shape this rule exists to report, and the `escape` half of
/// `SANITIZER` matched them both.
const ANTI_SANITIZER: &str = "unescape";

/// Properties whose assignment parses the right-hand side as HTML.
const HTML_PROPERTIES: [&str; 2] = ["innerHTML", "outerHTML"];

/// Functions that build a jQuery object, which is what makes a `.html()` on the
/// result the jQuery sink rather than any other method of that name.
const JQUERY: [&str; 2] = ["$", "jQuery"];

fn sanitizer() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(SANITIZER).expect("the sanitiser pattern compiles"))
}

/// How the value reaching the sink was built, which is all the evidence a
/// syntax tree offers.
#[derive(Clone, Copy)]
enum Kind {
    Variable,
    Property,
    Call,
    Template,
    Concatenation,
    Expression,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Variable => "a variable",
            Kind::Property => "a property",
            Kind::Call => "a call",
            Kind::Template => "a template with substitutions",
            Kind::Concatenation => "a concatenation",
            Kind::Expression => "an expression",
        }
    }
}

fn walk<'a>(node: Node<'a>, f: &mut impl FnMut(Node<'a>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

/// The named arguments of a call, comments dropped.
fn args<'a>(call: Node<'a>) -> Vec<Node<'a>> {
    let Some(list) = call.child_by_field_name("arguments").filter(|a| a.kind() == "arguments") else {
        return vec![];
    };
    let mut cursor = list.walk();
    list.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect()
}

/// Unwraps parentheses and the TypeScript casts that wrap an expression without
/// changing what it is.
fn unwrap<'a>(node: Node<'a>) -> Node<'a> {
    match node.kind() {
        "parenthesized_expression" | "as_expression" | "satisfies_expression" | "non_null_expression" => {
            node.named_child(0).map(unwrap).unwrap_or(node)
        }
        _ => node,
    }
}

fn is_addition(node: Node, src: &str) -> bool {
    node.kind() == "binary_expression" && node.child_by_field_name("operator").is_some_and(|o| text(o, src) == "+")
}

fn has_substitution(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|c| c.kind() == "template_substitution");
    found
}

/// The callee as written, whitespace collapsed so a call broken across lines
/// still reads as one name.
fn callee_text(call: Node, src: &str) -> String {
    let Some(f) = call.child_by_field_name("function") else { return String::new() };
    text(f, src).split_whitespace().collect::<Vec<_>>().join("")
}

/// Whether the node is a call to something named like a sanitiser. See the
/// module doc: the name is the whole of the evidence.
fn is_sanitizer_call(node: Node, src: &str) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let callee = callee_text(node, src);
    // Asked before the pattern, because the pattern says yes to this one.
    if callee.to_ascii_lowercase().contains(ANTI_SANITIZER) {
        return false;
    }
    sanitizer().is_match(&callee)
}

/// Whether the value is markup this file is responsible for: a fixed string
/// however it is quoted, a sanitiser's answer, or an addition of those.
fn is_safe(node: Node, src: &str) -> bool {
    let node = unwrap(node);
    match node.kind() {
        "string" | "number" => true,
        "template_string" => !has_substitution(node),
        _ if is_sanitizer_call(node, src) => true,
        _ if is_addition(node, src) => {
            let left = node.child_by_field_name("left");
            let right = node.child_by_field_name("right");
            match (left, right) {
                (Some(l), Some(r)) => is_safe(l, src) && is_safe(r, src),
                _ => false,
            }
        }
        _ => false,
    }
}

/// The shape of a value that is not safe, or `None` when it is.
fn unsafe_kind(node: Node, src: &str) -> Option<Kind> {
    let node = unwrap(node);
    if is_safe(node, src) {
        return None;
    }
    Some(match node.kind() {
        "template_string" => Kind::Template,
        "identifier" => Kind::Variable,
        "member_expression" | "subscript_expression" => Kind::Property,
        "call_expression" => Kind::Call,
        _ if is_addition(node, src) => Kind::Concatenation,
        _ => Kind::Expression,
    })
}

/// The value of an object literal's `__html` property, which is the one shape
/// `dangerouslySetInnerHTML` is documented to take.
fn html_property<'a>(node: Node<'a>, src: &'a str) -> Option<Node<'a>> {
    let node = unwrap(node);
    if node.kind() != "object" {
        return None;
    }
    let mut cursor = node.walk();
    let pairs: Vec<Node> = node.named_children(&mut cursor).filter(|c| c.kind() == "pair").collect();
    pairs.into_iter().find_map(|pair| {
        let key = pair.child_by_field_name("key")?;
        if text(key, src).trim_matches(['"', '\'']) != "__html" {
            return None;
        }
        pair.child_by_field_name("value")
    })
}

/// `<div dangerouslySetInnerHTML={{ __html: X }} />`. The attribute has no
/// fields in the grammar: its name is the first named child and its value the
/// second.
fn jsx_sink<'a>(node: Node<'a>, src: &'a str) -> Option<(&'static str, Node<'a>)> {
    if node.kind() != "jsx_attribute" {
        return None;
    }
    let name = node.named_child(0)?;
    if text(name, src) != "dangerouslySetInnerHTML" {
        return None;
    }
    let value = node.named_child(1).filter(|v| v.kind() == "jsx_expression")?;
    let inner = value.named_child(0)?;
    Some(("dangerouslySetInnerHTML", html_property(inner, src)?))
}

/// `el.innerHTML = X` and `el.outerHTML += X`.
fn assignment_sink<'a>(node: Node<'a>, src: &'a str) -> Option<(&'static str, Node<'a>)> {
    if !matches!(node.kind(), "assignment_expression" | "augmented_assignment_expression") {
        return None;
    }
    if node.kind() == "augmented_assignment_expression"
        && !node.child_by_field_name("operator").is_some_and(|o| text(o, src) == "+=")
    {
        return None;
    }
    let left = unwrap(node.child_by_field_name("left")?);
    if left.kind() != "member_expression" {
        return None;
    }
    let property = text(left.child_by_field_name("property")?, src);
    let named = HTML_PROPERTIES.iter().find(|p| **p == property)?;
    Some((named, node.child_by_field_name("right")?))
}

/// The bare name of a call: the identifier, or the property of a member call.
fn callee_name<'a>(call: Node<'a>, src: &'a str) -> Option<&'a str> {
    let f = call.child_by_field_name("function")?;
    match f.kind() {
        "identifier" => Some(text(f, src)),
        "member_expression" => Some(text(f.child_by_field_name("property")?, src)),
        _ => None,
    }
}

/// The object a member call hangs off, or `None` for a bare call.
fn callee_object<'a>(call: Node<'a>) -> Option<Node<'a>> {
    let f = call.child_by_field_name("function")?;
    if f.kind() != "member_expression" {
        return None;
    }
    f.child_by_field_name("object").map(unwrap)
}

/// Whether the receiver of a `.html()` call is a jQuery object, which is what
/// separates jQuery's sink from any other method of that name.
fn is_jquery_receiver(call: Node, src: &str) -> bool {
    let Some(object) = callee_object(call) else { return false };
    object.kind() == "call_expression"
        && object
            .child_by_field_name("function")
            .is_some_and(|f| f.kind() == "identifier" && JQUERY.contains(&text(f, src)))
}

/// `insertAdjacentHTML(pos, X)`, `document.write(X)`, and `$(...).html(X)`.
fn call_sink<'a>(call: Node<'a>, src: &'a str) -> Option<(&'static str, Node<'a>)> {
    if call.kind() != "call_expression" {
        return None;
    }
    match callee_name(call, src)? {
        "insertAdjacentHTML" => Some(("insertAdjacentHTML", *args(call).get(1)?)),
        // The object has to be `document`: `res.write` and `stream.write` are
        // not HTML sinks, and they are far commoner. See the module doc.
        "write" if callee_object(call).is_some_and(|o| text(o, src) == "document") => {
            Some(("document.write", *args(call).first()?))
        }
        "html" if is_jquery_receiver(call, src) => Some(("$().html", *args(call).first()?)),
        _ => None,
    }
}

fn scan(rule: &HtmlInjection, file: &ParsedFile) -> Vec<Finding> {
    let src = &file.source;
    let mut out: Vec<Finding> = Vec::new();
    walk(file.tree.root_node(), &mut |n: Node| {
        let Some((sink, value)) = jsx_sink(n, src).or_else(|| assignment_sink(n, src)).or_else(|| call_sink(n, src))
        else {
            return;
        };
        let Some(kind) = unsafe_kind(value, src) else { return };
        let evidence = format!("{sink} receives {}", kind.as_str());
        // The evidence joins the symbol in the anchor so that two sinks in one
        // function are two findings rather than one id written twice.
        let at = line(n);
        let anchor = format!("{}\x1f{}", anchor_for(file, at), evidence);
        let mut finding = finding_at(rule, &file.rel, line_span(file, at), &anchor, &evidence, FIX);
        finding.owasp = Some("A03:2021".to_string());
        finding.cwe = Some("CWE-79".to_string());
        out.push(finding);
    });
    out.sort_by_key(|f| (f.span.start_line, f.span.start_col));
    out
}

impl Rule for HtmlInjection {
    fn id(&self) -> &'static str {
        "html-injection"
    }
    fn description(&self) -> &'static str {
        "Unsanitised markup reaching an HTML sink"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Security
    }
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    /// Medium, and every finding keeps it: the shape says markup reaches a
    /// parser, not that anything hostile can reach the markup. See the module
    /// doc.
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}
