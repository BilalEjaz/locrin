mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::{Config, Framework};
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::express::cookie::ExpressCookieInsecure;
use locrin_rules::express::cors::ExpressCorsWildcardOnAuthenticated;
use locrin_rules::express::route_auth::ExpressRouteWithoutAuth;

const ROUTE_FIX: &str = "Add the auth middleware to the route or mount it with app.use before the router";
const CORS_FIX: &str = "Set origin to the allowed origins list and credentials: true only for them";
const COOKIE_FIX: &str = "Pass { httpOnly: true, secure: true, sameSite: \"lax\" }";

/// The config a repository writes to name the identifiers that mark a route as
/// authenticated. There is no default: no name can be guessed, so
/// `express-route-without-auth` says nothing until the repository names one.
fn auth(names: &[&str]) -> Config {
    let framework =
        Framework { auth_middleware: names.iter().map(|n| n.to_string()).collect(), ..Framework::default() };
    Config { framework, ..Config::default() }
}

#[test]
fn a_route_with_no_auth_middleware_is_flagged_and_a_public_path_is_not() {
    let out = run_on(Box::new(ExpressRouteWithoutAuth), &fixture("express", "flag"), &auth(&["requireAuth"]));
    assert_eq!(hits(&out), vec![("src/server.ts".to_string(), 9), ("src/server.ts".to_string(), 10)], "{out:?}");
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec!["GET /admin/users registered without requireAuth", "POST /admin/users registered without requireAuth",]
    );
    assert!(out.iter().all(|f| f.fix == ROUTE_FIX), "{:?}", out.first());
    assert!(
        out.iter().all(|f| f.category == Category::Security
            && f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A01:2021")
            && f.cwe.as_deref() == Some("CWE-306")),
        "{:?}",
        out.first()
    );
}

/// Two routes on one path are two findings, which is what the method in the
/// anchor buys: `GET /admin/users` and `POST /admin/users` are different
/// registrations and each needs its own decision.
#[test]
fn two_methods_on_one_path_are_two_findings() {
    let out = run_on(Box::new(ExpressRouteWithoutAuth), &fixture("express", "flag"), &auth(&["requireAuth"]));
    let ids: std::collections::HashSet<&str> = out.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids.len(), out.len(), "two registrations, two ids: {out:?}");
}

/// No name, no rule. The repository has to say which identifiers authenticate a
/// route before the rule can say one is missing.
#[test]
fn the_rule_is_silent_until_the_repository_names_its_middleware() {
    let out = run_on(Box::new(ExpressRouteWithoutAuth), &fixture("express", "flag"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

/// `app.use(requireAuth)` with no path mounts the middleware on everything
/// registered after it, and the public paths in the same file are skipped
/// whether or not it is there.
#[test]
fn a_global_mount_authenticates_every_route_after_it() {
    let out = run_on(Box::new(ExpressRouteWithoutAuth), &fixture("express", "clean"), &auth(&["requireAuth"]));
    assert!(out.is_empty(), "{out:?}");
}

/// A mount with a path covers the routes whose path it prefixes and no others.
#[test]
fn a_prefix_mount_covers_the_routes_under_it_only() {
    let out = run_on(Box::new(ExpressRouteWithoutAuth), &fixture("express", "edge"), &auth(&["requireAuth"]));
    assert_eq!(hits(&out), vec![("src/admin.ts".to_string(), 10)], "{out:?}");
    assert_eq!(out[0].evidence, "GET /jobs registered without requireAuth");
}

#[test]
fn wildcard_cors_beside_an_auth_middleware_and_on_a_mount_are_both_flagged() {
    let out =
        run_on(Box::new(ExpressCorsWildcardOnAuthenticated), &fixture("express", "flag"), &auth(&["requireAuth"]));
    assert_eq!(hits(&out), vec![("src/gateway.ts".to_string(), 8), ("src/server.ts".to_string(), 12)], "{out:?}");
    assert!(out.iter().all(|f| f.evidence == "wildcard CORS on an authenticated route"), "{:?}", out.first());
    assert!(out.iter().all(|f| f.fix == CORS_FIX), "{:?}", out.first());
    assert!(
        out.iter().all(|f| f.category == Category::Security
            && f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A05:2021")
            && f.cwe.as_deref() == Some("CWE-942")),
        "{:?}",
        out.first()
    );
}

/// An origin list is the fix, not the finding, and a wildcard on a file with no
/// authenticated route in it is somebody's public API.
#[test]
fn an_origin_list_is_not_a_wildcard() {
    let out =
        run_on(Box::new(ExpressCorsWildcardOnAuthenticated), &fixture("express", "clean"), &auth(&["requireAuth"]));
    assert!(out.is_empty(), "{out:?}");
    let unnamed = run_on(Box::new(ExpressCorsWildcardOnAuthenticated), &fixture("express", "flag"), &Config::default());
    assert!(unnamed.is_empty(), "no auth middleware named, no authenticated route: {unnamed:?}");
}

#[test]
fn a_cookie_set_without_the_flags_is_one_finding_per_missing_flag() {
    let out = run_on(Box::new(ExpressCookieInsecure), &fixture("express", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("src/server.ts".to_string(), 15), ("src/server.ts".to_string(), 15)], "{out:?}");
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(evidence, vec!["cookie session set without httpOnly", "cookie session set without secure"]);
    assert!(out.iter().all(|f| f.fix == COOKIE_FIX), "{:?}", out.first());
    let cwes: Vec<&str> = out.iter().map(|f| f.cwe.as_deref().unwrap()).collect();
    assert_eq!(cwes, vec!["CWE-1004", "CWE-614"]);
    assert!(
        out.iter().all(|f| f.category == Category::Security
            && f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A05:2021")),
        "{:?}",
        out.first()
    );
}

/// Both flags set is the whole fix, and `res.clearCookie` sets nothing.
#[test]
fn both_flags_and_a_cleared_cookie_are_quiet() {
    let out = run_on(Box::new(ExpressCookieInsecure), &fixture("express", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

/// A file that imports no express is not an express file, whatever its calls
/// look like. The cookie rule is the exception: `res.cookie(` is the shape
/// wherever it is written, and a handler in a file that takes its app from
/// somewhere else still sets the cookie.
#[test]
fn a_file_without_the_import_is_read_for_cookies_and_nothing_else() {
    let root = fixture("express", "edge");
    let routes = run_on(Box::new(ExpressRouteWithoutAuth), &root, &auth(&["requireAuth"]));
    assert!(routes.iter().all(|f| f.file != "src/plain.ts"), "{routes:?}");
    let cors = run_on(Box::new(ExpressCorsWildcardOnAuthenticated), &root, &auth(&["requireAuth"]));
    assert!(cors.is_empty(), "{cors:?}");
    let cookies = run_on(Box::new(ExpressCookieInsecure), &root, &Config::default());
    assert_eq!(hits(&cookies), vec![("src/plain.ts".to_string(), 8), ("src/plain.ts".to_string(), 8)], "{cookies:?}");
    let evidence: Vec<&str> = cookies.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(evidence, vec!["cookie session set without httpOnly", "cookie session set without secure"]);
}
