//! Flags a cookie set without `httpOnly` or without `secure`.
//!
//! The two flags defend against two different attacks and neither substitutes
//! for the other. Without `httpOnly` any script on the page can read the cookie
//! through `document.cookie`, which turns a cross-site scripting bug into a
//! stolen session. Without `secure` the browser sends the cookie over plain
//! HTTP, so anybody between the user and the server reads it once. Both are one
//! word in an options object, both are off by default, and neither absence
//! changes anything a test would notice.
//!
//! So the rule reports one finding per missing flag rather than one per cookie:
//! they are two decisions, they are filed under two weaknesses (`CWE-1004` for
//! `httpOnly` and `CWE-614` for `secure`), and a repository that has fixed one
//! should see the other still standing rather than watch a single finding
//! change its wording.
//!
//! This is the one Express rule that runs in a file with no `express` import,
//! because `res.cookie(` is the shape wherever it is written: a route handler
//! exported from a file that never mentions Express sets the cookie exactly as
//! the file that built the app does.
//!
//! **Unmeasured on the corpus.** None of the five repositories the engine is
//! measured against uses Express, so this rule has no precision sample. See the
//! module doc.
//!
//! Blind spots:
//!
//! - **The receiver is not checked.** Any `<identifier>.cookie(name, value)` is
//!   read, because the response is called `res` in most handlers, `response` in
//!   others and `reply` in a few, and demanding one name would miss the other
//!   two. The cost is that a two-argument `.cookie(...)` on something else
//!   entirely would be read as a cookie; the second argument and the file gate
//!   are what keep that rare.
//! - **`res.clearCookie` is not a cookie being set**, so it is not read. It
//!   takes the same options object and deletes rather than stores.
//! - **Options this file cannot read are left alone.** `res.cookie(n, v, opts)`
//!   where `opts` is a variable is quiet: the rule reports the options it can
//!   quote, the same way `express-cors-wildcard-on-authenticated` reports the
//!   origins it can quote.
//! - **A flag whose value is neither `true` nor `false` counts as set.**
//!   `secure: process.env.NODE_ENV === "production"` is how a repository writes
//!   a cookie that is secure in production and readable over `http://localhost`
//!   in development, and it is not a missing flag. A flag is missing when it is
//!   absent from the object or written `false`. (The task brief said "lacking
//!   `httpOnly: true`", which read literally would report that idiom; the
//!   deviation is recorded in the plan.)
//! - **The default is not read.** A framework or wrapper that sets the flags
//!   for every cookie makes each call site quiet about them, and the rule
//!   reports the call site. `locrin:allow` is the answer where that is true.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use tree_sitter::Node;

use super::{args, imports_express, property, string_value, unwrap, walk};
use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct ExpressCookieInsecure;

const FIX: &str = "Pass { httpOnly: true, secure: true, sameSite: \"lax\" }";

/// The call shape that puts a cookie in the response, spelled the way the
/// second door reads a file: any file holding these characters is setting one.
const MARKER: &str = "res.cookie(";

/// The two flags, each with the weakness a missing one is filed under, in the
/// order they are reported.
const FLAGS: [(&str, &str); 2] = [("httpOnly", "CWE-1004"), ("secure", "CWE-614")];

/// Whether the call is `<identifier>.cookie(name, value, ...)`, and its
/// arguments if it is. Two arguments at least: a one-argument `.cookie(x)` is a
/// getter or somebody else's API, not a cookie being set.
fn cookie_call<'a>(call: Node<'a>, src: &str) -> Option<Vec<Node<'a>>> {
    if call.kind() != "call_expression" {
        return None;
    }
    let f = call.child_by_field_name("function")?;
    if f.kind() != "member_expression" || f.child_by_field_name("object")?.kind() != "identifier" {
        return None;
    }
    if text(f.child_by_field_name("property")?, src) != "cookie" {
        return None;
    }
    let arguments = args(call);
    (arguments.len() >= 2).then_some(arguments)
}

/// The flags an options argument leaves missing. `None` means the options are
/// there but unreadable, which is not an answer and not a finding.
///
/// Absent options miss both: `res.cookie(name, value)` is a cookie with no
/// flags on it at all.
fn missing<'a>(options: Option<Node<'a>>, src: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let Some(options) = options else { return Some(FLAGS.to_vec()) };
    let options = unwrap(options);
    if options.kind() != "object" {
        return None;
    }
    Some(
        FLAGS
            .iter()
            .filter(|(flag, _)| match property(options, src, flag).map(unwrap) {
                // Absent, or turned off in as many words.
                None => true,
                Some(value) => text(value, src) == "false",
            })
            .copied()
            .collect(),
    )
}

/// The cookie's name as the evidence says it: the literal when there is one,
/// otherwise the expression as written, so a reader can find the call.
fn cookie_name(node: Node, src: &str) -> String {
    string_value(node, src).unwrap_or_else(|| text(node, src).split_whitespace().collect::<Vec<_>>().join(" "))
}

fn scan(rule: &ExpressCookieInsecure, file: &ParsedFile) -> Vec<Finding> {
    let src = &file.source;
    let mut out: Vec<Finding> = Vec::new();
    walk(file.tree.root_node(), &mut |n: Node| {
        let Some(arguments) = cookie_call(n, src) else { return };
        let Some(missing) = missing(arguments.get(2).copied(), src) else { return };
        let name = cookie_name(arguments[0], src);
        let at = line(n);
        for (flag, cwe) in missing {
            // The name and the flag are both in the anchor: two cookies set in
            // one function are two findings, and so are the two flags of one
            // cookie, which is what lets a repository fix them one at a time.
            let anchor = format!("cookie\x1f{name}\x1f{flag}");
            let mut f = finding_at(
                rule,
                &file.rel,
                line_span(file, at),
                &anchor,
                &format!("cookie {name} set without {flag}"),
                FIX,
            );
            f.owasp = Some("A05:2021".to_string());
            f.cwe = Some(cwe.to_string());
            out.push(f);
        }
    });
    out.sort_by_key(|f| f.span.start_line);
    out
}

impl Rule for ExpressCookieInsecure {
    fn id(&self) -> &'static str {
        "express-cookie-insecure"
    }
    fn description(&self) -> &'static str {
        "Cookie set without httpOnly or secure"
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
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx)
            .filter(|f| imports_express(f) || f.source.contains(MARKER))
            .flat_map(|file| scan(self, file))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> ParsedFile {
        locrin_core::parse::parse_source(std::path::Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    fn evidence(source: &str) -> Vec<String> {
        scan(&ExpressCookieInsecure, &parsed(source)).iter().map(|f| f.evidence.clone()).collect()
    }

    #[test]
    fn no_options_misses_both_flags_and_one_flag_leaves_the_other() {
        assert_eq!(
            evidence("res.cookie(\"session\", token);\n"),
            vec!["cookie session set without httpOnly", "cookie session set without secure"]
        );
        assert_eq!(
            evidence("res.cookie(\"session\", token, { httpOnly: true });\n"),
            vec!["cookie session set without secure"]
        );
        assert_eq!(
            evidence("res.cookie(\"session\", token, { secure: true, sameSite: \"lax\" });\n"),
            vec!["cookie session set without httpOnly"]
        );
        assert!(evidence("res.cookie(\"session\", token, { httpOnly: true, secure: true });\n").is_empty());
        assert_eq!(
            evidence("res.cookie(\"session\", token, { httpOnly: true, secure: false });\n"),
            vec!["cookie session set without secure"],
            "off in as many words is off"
        );
    }

    /// See the module doc: a flag the file cannot evaluate is not a missing
    /// flag, and options it cannot read are not an answer at all.
    #[test]
    fn a_computed_flag_and_an_unreadable_options_object_are_left_alone() {
        assert!(evidence(
            "res.cookie(\"s\", t, { httpOnly: true, secure: process.env.NODE_ENV === \"production\" });\n"
        )
        .is_empty());
        assert!(evidence("res.cookie(\"s\", t, options);\n").is_empty());
    }

    #[test]
    fn clearing_a_cookie_and_a_one_argument_call_are_not_cookies_being_set() {
        assert!(evidence("res.clearCookie(\"session\");\n").is_empty());
        assert!(evidence("res.clearCookie(\"session\", { httpOnly: true });\n").is_empty());
        assert!(evidence("jar.cookie(\"session\");\n").is_empty());
    }

    /// The receiver is any name, and a cookie whose name is an expression is
    /// still reported, quoted as written.
    #[test]
    fn any_receiver_and_a_computed_name() {
        assert_eq!(
            evidence("reply.cookie(name, token, { secure: true });\n"),
            vec!["cookie name set without httpOnly"]
        );
        assert_eq!(
            evidence("response.cookie(`${prefix}_session`, token, { httpOnly: true });\n"),
            vec!["cookie `${prefix}_session` set without secure"]
        );
    }
}
