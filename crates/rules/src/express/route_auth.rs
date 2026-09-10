//! Flags an Express route registered with none of the repository's auth
//! middlewares on it.
//!
//! A missing middleware is the quietest possible bug. The route works, the
//! tests pass, the handler returns the data it was written to return, and the
//! only thing wrong with it is that it returns the data to anybody who asks.
//! Nothing in the type system, the linter or the test suite has an opinion,
//! because the argument list of `app.get` is a list of functions and one fewer
//! function is a valid list.
//!
//! The rule cannot guess the name. There is no Express convention: one
//! repository writes `requireAuth`, the next `authenticate`, the next
//! `ensureSession`, and a rule that guessed would either miss the repository it
//! was pointed at or report every route in it. So `framework.auth_middleware`
//! (spec 7.5) is the whole input, it is empty by default, and the rule produces
//! nothing at all until the repository names at least one identifier. That is
//! the one rule in the engine whose behaviour is switched off by an absent
//! config rather than by a disabled one, and it is deliberate: silence is the
//! honest answer to a question the repository has not answered.
//!
//! **Unmeasured on the corpus.** None of the five repositories the engine is
//! measured against uses Express, so this rule has no precision sample. See the
//! module doc.
//!
//! Blind spots on top of the ones in the module doc:
//!
//! - **Public paths are skipped by name.** A path holding `health`, `status`,
//!   `ping`, `login`, `signin`, `signup`, `register`, `logout`, `webhook`,
//!   `callback`, `oauth`, `public` or `docs` is meant to be reachable without a
//!   session, and reporting it teaches a reader to ignore the rule. The cost is
//!   that a genuinely private `/admin/status` is skipped with them: a word in a
//!   path is a weak signal, and it is being spent on quiet rather than on
//!   coverage.
//! - **`use` is never reported.** A mount is middleware, not an endpoint, and
//!   the thing it is missing is the thing it might itself be.
//! - **A one-argument `app.get(...)` is not read.** Express overloads `app.get`
//!   with the getter half of its settings API, so `app.get("env")` and
//!   `app.get("trust proxy")` read a setting rather than register a route. A
//!   registration takes a path and at least one handler, so a call with fewer
//!   than two arguments is skipped. The cost is nil: a route with no handler
//!   answers nothing.
//! - **Authorisation is not authentication.** A route carrying `requireAuth`
//!   passes whatever it does next with the identity it established. Whether the
//!   right user is allowed at the right row is not a question a middleware list
//!   answers.

use std::sync::OnceLock;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use regex::Regex;

use super::{args, auth_mounts, imports_express, is_authenticated, registrations};
use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct ExpressRouteWithoutAuth;

const FIX: &str = "Add the auth middleware to the route or mount it with app.use before the router";

/// The words that make a path public. See the module doc for what this buys and
/// what it costs.
fn public_path() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new("(?i)(health|status|ping|login|signin|signup|register|logout|webhook|callback|oauth|public|docs)")
            .unwrap()
    })
}

fn scan(rule: &ExpressRouteWithoutAuth, file: &ParsedFile, names: &[String]) -> Vec<Finding> {
    let src = &file.source;
    let mounts = auth_mounts(file, names);
    let mut out = Vec::new();
    for reg in registrations(file) {
        // A mount is middleware, not an endpoint.
        if reg.method == "use" {
            continue;
        }
        // `app.get("env")` is not a route. Express overloads `app.get` with the
        // getter half of its settings API, and a settings read takes exactly
        // one argument where a route registration takes a path and at least one
        // handler. One argument is the getter every time.
        if args(reg.call).len() < 2 {
            continue;
        }
        let Some(path) = reg.path.clone() else { continue };
        if public_path().is_match(&path) || is_authenticated(&reg, src, names, &mounts) {
            continue;
        }
        let method = reg.method_name();
        // The method joins the path in the anchor: `GET /users` and
        // `POST /users` are two registrations, each needing its own decision,
        // and an anchor on the path alone would give them one id between them.
        let anchor = format!("route\x1f{method} {path}");
        let mut f = finding_at(
            rule,
            &file.rel,
            line_span(file, reg.line),
            &anchor,
            &format!("{method} {path} registered without {}", names.join(", ")),
            FIX,
        );
        f.owasp = Some("A01:2021".to_string());
        f.cwe = Some("CWE-306".to_string());
        out.push(f);
    }
    out
}

impl Rule for ExpressRouteWithoutAuth {
    fn id(&self) -> &'static str {
        "express-route-without-auth"
    }
    fn description(&self) -> &'static str {
        "Express route registered without an auth middleware"
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
        // The repository has not said what authenticates a route, so the rule
        // has nothing to say about a route that is missing one.
        if names.is_empty() {
            return Ok(Vec::new());
        }
        Ok(clean_files(ctx).filter(|f| imports_express(f)).flat_map(|file| scan(self, file, names)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_word_is_matched_anywhere_in_the_path() {
        for path in ["/health", "/healthz", "/api/v1/status", "/auth/login", "/oauth/callback", "/docs", "/public/x"] {
            assert!(public_path().is_match(path), "{path}");
        }
        for path in ["/admin/users", "/orders", "/api/v1/reports", "/profile"] {
            assert!(!public_path().is_match(path), "{path}");
        }
        // The cost, said out loud: a private path carrying a public word is
        // skipped with them.
        assert!(public_path().is_match("/admin/status"), "the known cost of matching a word anywhere");
    }

    /// The evidence names every middleware the repository listed, so a reader
    /// who has two knows both are absent rather than guessing which was meant.
    #[test]
    fn the_evidence_names_the_whole_list() {
        let names = vec!["requireAuth".to_string(), "authenticate".to_string()];
        let file = locrin_core::parse::parse_source(
            std::path::Path::new("src/a.ts"),
            "src/a.ts",
            "app.get(\"/admin\", h);\n".into(),
        )
        .unwrap();
        let out = scan(&ExpressRouteWithoutAuth, &file, &names);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].evidence, "GET /admin registered without requireAuth, authenticate");
        assert_eq!(out[0].fix, FIX);
    }

    /// Express's settings getter shares the name of its route registrar. One
    /// argument is the getter, and a getter has no handler to be missing an
    /// auth middleware in front of.
    #[test]
    fn a_settings_read_is_not_a_route() {
        let names = vec!["requireAuth".to_string()];
        let file = locrin_core::parse::parse_source(
            std::path::Path::new("src/a.ts"),
            "src/a.ts",
            "const mode = app.get(\"env\");\nconst proxy = app.get(\"trust proxy\");\napp.get(\"/admin\", h);\n".into(),
        )
        .unwrap();
        let out = scan(&ExpressRouteWithoutAuth, &file, &names);
        let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
        assert_eq!(evidence, vec!["GET /admin registered without requireAuth"]);
    }
}
