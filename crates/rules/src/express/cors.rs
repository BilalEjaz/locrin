//! Flags a wildcard CORS policy on a route that carries authentication.
//!
//! `cors()` with nothing configured answers every origin, which is the right
//! default for a public read-only API and the wrong one for anything a session
//! reaches. Put beside an auth middleware it says: any page on the internet may
//! ask this endpoint for its data, and the browser will attach whatever it has
//! to the request. The two settings are individually reasonable and only
//! dangerous together, which is why neither is worth a rule on its own.
//!
//! Two shapes are read, and both need an authenticated route to mean anything:
//!
//! - **On the route.** `app.get("/reports", requireAuth, cors(), handler)`. The
//!   auth middleware and the wildcard are arguments of the same call, so the
//!   rule can see both at once and needs nothing else.
//! - **On a mount.** `app.use(cors({ origin: "*" }))` in a file that registers
//!   at least one authenticated route. A mount applies to everything after it,
//!   so a wildcard mounted in a file whose routes are behind a session reaches
//!   those routes.
//!
//! A wildcard is `cors()` with no arguments, `origin: "*"`, or `origin: true`,
//! which are the three ways the `cors` package is told to reflect whatever
//! origin asked.
//!
//! **Unmeasured on the corpus.** None of the five repositories the engine is
//! measured against uses Express, so this rule has no precision sample. See the
//! module doc.
//!
//! Blind spots on top of the ones in the module doc:
//!
//! - **An origin the file cannot read is not a wildcard.** `cors(corsOptions)`
//!   and `cors({ origin: allowed })` are quiet, whatever `allowed` turns out to
//!   hold: the rule reports the policy it can quote.
//! - **`cors({ credentials: true })` with no `origin` at all is quiet**, even
//!   though the package's own default origin is `*`. The rule reports what the
//!   file says rather than what the dependency defaults to, because the default
//!   is the dependency's to change and the reader has to be able to see the
//!   finding in the line.
//! - **The auth middleware still has to be named.** With no
//!   `framework.auth_middleware` no route is authenticated, so this rule is
//!   silent for the same reason `express-route-without-auth` is.
//! - **A wildcard mount and an authenticated route in two different files do
//!   not meet.** This is a File rule; the mount is judged by the file it sits
//!   in.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use tree_sitter::Node;

use super::{
    args, auth_mounts, imports_express, is_auth, is_authenticated, property, registrations, string_value, unwrap, walk,
    Registration,
};
use crate::{anchor_for, clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct ExpressCorsWildcardOnAuthenticated;

const EVIDENCE: &str = "wildcard CORS on an authenticated route";
const FIX: &str = "Set origin to the allowed origins list and credentials: true only for them";

/// Whether a call is a `cors(...)` that answers any origin: no arguments at all,
/// or an options object saying `origin: "*"` or `origin: true`.
fn is_wildcard_cors(call: Node, src: &str) -> bool {
    if call.kind() != "call_expression" {
        return false;
    }
    let Some(f) = call.child_by_field_name("function") else { return false };
    let name = match f.kind() {
        "identifier" => text(f, src),
        "member_expression" => match f.child_by_field_name("property") {
            Some(p) => text(p, src),
            None => return false,
        },
        _ => return false,
    };
    if name != "cors" {
        return false;
    }
    let arguments = args(call);
    let Some(first) = arguments.first() else { return true };
    match property(*first, src, "origin").map(unwrap) {
        Some(origin) => text(origin, src) == "true" || string_value(origin, src).as_deref() == Some("*"),
        None => false,
    }
}

/// The registration a node is a direct argument of, if any. A `cors()` nested
/// inside an options object or a helper call is not on the route: the rule
/// reads the argument list, not the expression tree under it.
fn enclosing<'a, 'r>(node: Node<'a>, regs: &'r [Registration<'a>]) -> Option<&'r Registration<'a>> {
    let list = node.parent().filter(|p| p.kind() == "arguments")?;
    let call = list.parent()?;
    regs.iter().find(|r| r.call.id() == call.id())
}

fn scan(rule: &ExpressCorsWildcardOnAuthenticated, file: &ParsedFile, names: &[String]) -> Vec<Finding> {
    let src = &file.source;
    let regs = registrations(file);
    let mounts = auth_mounts(file, names);
    // "At least one authenticated route": a registration that is not a mount
    // and that carries, or sits under, one of the named middlewares.
    let any_authenticated = regs.iter().filter(|r| r.method != "use").any(|r| is_authenticated(r, src, names, &mounts));

    let mut out: Vec<Finding> = Vec::new();
    walk(file.tree.root_node(), &mut |n: Node| {
        if !is_wildcard_cors(n, src) {
            return;
        }
        let Some(reg) = enclosing(n, &regs) else { return };
        // Beside a middleware in the same call, or mounted in a file whose
        // routes are behind one. Either way the wildcard reaches a session.
        let beside_auth = args(reg.call).iter().any(|a| is_auth(*a, src, names));
        if !(beside_auth || (reg.method == "use" && any_authenticated)) {
            return;
        }
        let at = line(n);
        // The registration names the finding: `GET /reports` for a route, and
        // for a mount with no path the symbol the line sits in, which is what
        // every other rule anchors on.
        let target = match &reg.path {
            Some(path) => format!("{} {path}", reg.method_name()),
            None => format!("{} {}", reg.method_name(), anchor_for(file, at)),
        };
        let mut f = finding_at(rule, &file.rel, line_span(file, at), &format!("cors\x1f{target}"), EVIDENCE, FIX);
        f.owasp = Some("A05:2021".to_string());
        f.cwe = Some("CWE-942".to_string());
        out.push(f);
    });
    out.sort_by_key(|f| f.span.start_line);
    out
}

impl Rule for ExpressCorsWildcardOnAuthenticated {
    fn id(&self) -> &'static str {
        "express-cors-wildcard-on-authenticated"
    }
    fn description(&self) -> &'static str {
        "Wildcard CORS on an authenticated Express route"
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
        let names = &ctx.config.framework.auth_middleware;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        Ok(clean_files(ctx).filter(|f| imports_express(f)).flat_map(|file| scan(self, file, names)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> ParsedFile {
        locrin_core::parse::parse_source(std::path::Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    /// The three wildcard spellings, and the two shapes that are not wildcards.
    #[test]
    fn a_wildcard_is_no_options_or_an_origin_that_reflects_anything() {
        let cases = [
            ("cors()", true),
            ("cors({ origin: \"*\" })", true),
            ("cors({ origin: true })", true),
            ("cors({ origin: \"*\", credentials: true })", true),
            ("cors({ origin: [\"https://a.example.com\"] })", false),
            ("cors({ origin: allowed })", false),
            ("cors(corsOptions)", false),
            ("cors({ credentials: true })", false),
            ("helmet()", false),
        ];
        for (source, expected) in cases {
            let file = parsed(&format!("const x = {source};\n"));
            let mut found = false;
            walk(file.tree.root_node(), &mut |n: Node| {
                if n.kind() == "call_expression" && is_wildcard_cors(n, &file.source) {
                    found = true;
                }
            });
            assert_eq!(found, expected, "{source}");
        }
    }

    /// A wildcard on a route with no middleware on it, in a file with no
    /// authenticated route in it, is somebody's public API and not a finding.
    #[test]
    fn a_wildcard_without_authentication_anywhere_is_a_public_api() {
        let names = vec!["requireAuth".to_string()];
        let file = parsed("app.use(cors());\napp.get(\"/prices\", listPrices);\n");
        assert!(scan(&ExpressCorsWildcardOnAuthenticated, &file, &names).is_empty());

        // One authenticated route in the file is what makes the mount reach a
        // session.
        let file = parsed("app.use(cors());\napp.get(\"/prices\", listPrices);\napp.get(\"/me\", requireAuth, me);\n");
        let out = scan(&ExpressCorsWildcardOnAuthenticated, &file, &names);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].span.start_line, 1);
    }

    /// A `cors()` that is not an argument of a registration is not on a route.
    #[test]
    fn a_wildcard_that_is_not_on_a_registration_is_not_read() {
        let names = vec!["requireAuth".to_string()];
        let file = parsed("const options = { middleware: cors() };\napp.get(\"/me\", requireAuth, me);\n");
        assert!(scan(&ExpressCorsWildcardOnAuthenticated, &file, &names).is_empty());
    }
}
