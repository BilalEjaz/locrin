//! The three rules that read an Express application the way a request reaches
//! it: which routes anybody can call, which of them answer any origin, and
//! which cookies leave the browser willing to hand them over.
//!
//! Express is configuration written as code. A route is a call, the
//! authentication on it is an argument, and the difference between a private
//! endpoint and a public one is whether somebody remembered to put the
//! middleware in the list. Nothing in the language marks the omission, no type
//! catches it, and the endpoint works either way, which is what makes it worth
//! a rule.
//!
//! All three are File scope, Security, High severity and High confidence, and
//! all three run only in a file that imports `express` (an `import` or a
//! `require` with the specifier `express`), because the shapes they match are
//! ordinary method calls that mean something else everywhere else:
//! `router.get(key)` on a cache and `app.use(x)` on a plugin host are not
//! routes. [`cookie`] is the one exception, and it takes a second door: a file
//! holding `res.cookie(` is setting a cookie whether or not it is the file that
//! built the app.
//!
//! **Unmeasured on the corpus.** None of the five repositories the engine is
//! measured against uses Express, so the spec 10.2 precision gate has no sample
//! here: the corpus run confirms only that the rules stay silent where there is
//! no Express, which is the one thing it can confirm. Each rule's own doc
//! repeats this. The fixtures are what pin the behaviour, and the first real
//! Express repository is the measurement.
//!
//! What the three share is here: the file gate, the route registrations a file
//! makes, and what an auth middleware looks like as an argument. The blind
//! spots they share come with it:
//!
//! - **The receiver of a registration is any identifier, and only an
//!   identifier.** `app`, `router`, `api` and `server` are the usual names and
//!   the rules do not care which; but `this.app.get(...)` and
//!   `getApp().get(...)` are not read, because the receiver is not a name this
//!   file can hold on to.
//! - **A path is a string literal.** A route registered with a regular
//!   expression, an array of paths, or a variable has no path here: the rules
//!   report the registrations whose path they can quote.
//! - **A mount's prefix is compared as text, one whole segment at a time.**
//!   `app.use("/admin", requireAuth)` covers `app.get("/admin/users", ...)`
//!   because the second path continues the first at a slash, and it does not
//!   cover `/administration`, which merely starts with the same characters. It
//!   does not cover the commoner Express idiom, a `Router` mounted
//!   at `/admin` whose own routes are registered as `/users`: the router's paths
//!   do not carry the prefix, and joining them would mean following the router
//!   object across files, which is release two's work.
//! - **Order in the file is order at runtime.** A mount covers what is
//!   registered after it, which is how Express itself works, and a mount added
//!   by a function called from somewhere else is invisible.

pub mod cookie;
pub mod cors;
pub mod route_auth;

use locrin_core::imports;
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use tree_sitter::Node;

/// The module specifier that makes a file an Express file.
const EXPRESS: &str = "express";

/// The methods a route is registered with. `use` is here because a mount is
/// read for the middleware it carries; it is never itself reported as a route.
pub(crate) const ROUTE_METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "all", "use"];

/// Whether the file brings `express` in, by an import or a `require`.
///
/// [`imports::extract`] reads both, and it reads only literal specifiers, which
/// is the right answer here: a module name built at runtime is not one a rule
/// should guess at.
pub(crate) fn imports_express(file: &ParsedFile) -> bool {
    imports::extract(file).iter().any(|i| i.specifier == EXPRESS)
}

pub(crate) fn walk<'a>(node: Node<'a>, f: &mut impl FnMut(Node<'a>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

/// Unwraps the parentheses and TypeScript casts that wrap an expression without
/// changing what it is, so `(requireAuth as Handler)` is still `requireAuth`.
pub(crate) fn unwrap(node: Node<'_>) -> Node<'_> {
    match node.kind() {
        "parenthesized_expression" | "as_expression" | "satisfies_expression" | "non_null_expression" => {
            node.named_child(0).map(unwrap).unwrap_or(node)
        }
        _ => node,
    }
}

/// The named arguments of a call, comments dropped.
pub(crate) fn args(call: Node<'_>) -> Vec<Node<'_>> {
    let Some(list) = call.child_by_field_name("arguments").filter(|a| a.kind() == "arguments") else {
        return vec![];
    };
    let mut cursor = list.walk();
    list.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect()
}

/// The value of a plain string literal, or `None` for anything else. A template
/// literal is not read: an interpolated path is not a path this file can quote.
pub(crate) fn string_value(node: Node, src: &str) -> Option<String> {
    let node = unwrap(node);
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let parts: Vec<Node> = node.named_children(&mut cursor).collect();
    match parts.as_slice() {
        [] => Some(String::new()),
        [f] if f.kind() == "string_fragment" => Some(text(*f, src).to_string()),
        _ => None,
    }
}

/// The value of an object literal's property, by key. The key is read quoted or
/// bare, because `{ "origin": "*" }` and `{ origin: "*" }` are the same object.
pub(crate) fn property<'a>(object: Node<'a>, src: &'a str, key: &str) -> Option<Node<'a>> {
    let object = unwrap(object);
    if object.kind() != "object" {
        return None;
    }
    let mut cursor = object.walk();
    let pairs: Vec<Node> = object.named_children(&mut cursor).filter(|c| c.kind() == "pair").collect();
    pairs
        .into_iter()
        .find(|pair| {
            pair.child_by_field_name("key").is_some_and(|k| text(k, src).trim_matches(|c| c == '"' || c == '\'') == key)
        })?
        .child_by_field_name("value")
}

/// A call the rules read as a registration on an Express application: the
/// receiver's method, the path when it is a literal, and where it sits.
pub(crate) struct Registration<'a> {
    /// The method as written, e.g. `get` or `use`.
    pub method: String,
    /// The literal first argument, or `None` when the call has no string path.
    /// `app.use(cors())` is a registration with no path.
    pub path: Option<String>,
    pub call: Node<'a>,
    pub line: u32,
    /// The byte the call starts at, which is how "earlier in the file" is
    /// decided: a mount covers what is registered after it.
    pub start: usize,
}

impl Registration<'_> {
    /// The method as the evidence spells it. An HTTP method is read in upper
    /// case everywhere else a reader meets one, in a log line and in a
    /// specification, so it is read that way here.
    pub fn method_name(&self) -> String {
        self.method.to_uppercase()
    }
}

/// Whether a call is `<identifier>.<method>(...)` for one of [`ROUTE_METHODS`],
/// and the method if it is.
fn registration_method(call: Node, src: &str) -> Option<String> {
    if call.kind() != "call_expression" {
        return None;
    }
    let f = call.child_by_field_name("function")?;
    if f.kind() != "member_expression" || f.child_by_field_name("object")?.kind() != "identifier" {
        return None;
    }
    let property = text(f.child_by_field_name("property")?, src);
    ROUTE_METHODS.contains(&property).then(|| property.to_string())
}

/// Every registration in the file, in source order.
pub(crate) fn registrations(file: &ParsedFile) -> Vec<Registration<'_>> {
    let src = &file.source;
    let mut out: Vec<Registration> = Vec::new();
    walk(file.tree.root_node(), &mut |n: Node| {
        let Some(method) = registration_method(n, src) else { return };
        let path = args(n).first().and_then(|a| string_value(*a, src));
        out.push(Registration { method, path, call: n, line: line(n), start: n.start_byte() });
    });
    out.sort_by_key(|r| r.start);
    out
}

/// The route registration a node sits inside, spelled `GET /admin/users`, or
/// `None` when there is none or when the nearest one registers no literal path.
///
/// The climb stops at the first registration whose *arguments* hold the node,
/// which is the handler it was passed to. A registration reached any other way
/// is not one this node is inside: the receiver of `app.get(...)` is `app`, not
/// a handler.
///
/// This is a name for an anonymous handler, which is what most handlers are. A
/// finding inside one has no enclosing symbol to anchor on, and the fallback
/// (the text of the line the finding sits on) is the same text in every handler
/// that writes the same call, so two of them would share one finding id.
pub(crate) fn enclosing_registration(node: Node, src: &str) -> Option<String> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.child_by_field_name("arguments").is_some_and(|a| a.id() == current.id()) {
            if let Some(method) = registration_method(parent, src) {
                let path = args(parent).first().and_then(|a| string_value(*a, src))?;
                return Some(format!("{} {path}", method.to_uppercase()));
            }
        }
        current = parent;
    }
    None
}

/// Whether an argument names one of the repository's auth middlewares.
///
/// Four shapes, and the compositions of them: the bare identifier
/// (`requireAuth`), a call that builds one (`requireAuth()`,
/// `requireRole("admin")`), a member whose property is the name
/// (`auth.requireAuth`), and an array of any of those. A call whose callee is a
/// member (`auth.requireAuth()`) is all of them at once, which is why the test
/// recurses rather than listing the cases.
///
/// The array arm is Express's own documented shape for a middleware chain:
/// `app.post("/orders", [requireAuth, validate], create)` passes one argument
/// where the flat form passes two, and Express flattens it before running it.
/// Any element authenticating authenticates the array, for the same reason any
/// argument authenticates the call.
pub(crate) fn is_auth(node: Node, src: &str, names: &[String]) -> bool {
    let node = unwrap(node);
    match node.kind() {
        "identifier" => names.iter().any(|n| n == text(node, src)),
        "member_expression" => {
            node.child_by_field_name("property").is_some_and(|p| names.iter().any(|n| n == text(p, src)))
        }
        "call_expression" => node.child_by_field_name("function").is_some_and(|f| is_auth(f, src, names)),
        "array" => {
            // Two statements, not one expression: the iterator borrows
            // `cursor`, so it has to be dropped before `cursor` is.
            let mut cursor = node.walk();
            let elements: Vec<Node> = node.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect();
            elements.into_iter().any(|e| is_auth(e, src, names))
        }
        _ => false,
    }
}

/// A `use` registration carrying an auth middleware: the path it is mounted at
/// (empty for a mount with no path, which covers everything) and where it sits.
pub(crate) struct Mount {
    pub prefix: String,
    pub start: usize,
}

/// Every auth mount in the file. A `use` call is a mount when any of its
/// arguments names an auth middleware; the path, when it has one, is the prefix
/// the mount covers.
pub(crate) fn auth_mounts(file: &ParsedFile, names: &[String]) -> Vec<Mount> {
    let src = &file.source;
    registrations(file)
        .iter()
        .filter(|r| r.method == "use")
        .filter(|r| args(r.call).iter().any(|a| is_auth(*a, src, names)))
        .map(|r| Mount { prefix: r.path.clone().unwrap_or_default(), start: r.start })
        .collect()
}

/// Whether a mount's prefix covers a path. A mount covers the prefix itself and
/// everything below it as a path segment, which is how Express matches: `/api`
/// covers `/api` and `/api/users` and does not cover `/apiary`. A prefix that
/// is empty, or that is only slashes, is a mount with no path and covers
/// everything.
pub(crate) fn covers(prefix: &str, path: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        return true;
    }
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

/// Whether a registration is authenticated: a middleware in its own argument
/// list after the path, or a mount earlier in the file whose prefix covers its
/// path.
pub(crate) fn is_authenticated(reg: &Registration, src: &str, names: &[String], mounts: &[Mount]) -> bool {
    if args(reg.call).iter().skip(1).any(|a| is_auth(*a, src, names)) {
        return true;
    }
    let path = reg.path.clone().unwrap_or_default();
    mounts.iter().any(|m| m.start < reg.start && covers(&m.prefix, &path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> ParsedFile {
        locrin_core::parse::parse_source(std::path::Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    #[test]
    fn an_import_and_a_require_both_open_the_gate_and_a_near_name_does_not() {
        assert!(imports_express(&parsed("import express from \"express\";\n")));
        assert!(imports_express(&parsed("const express = require(\"express\");\n")));
        assert!(imports_express(&parsed("import { Router } from \"express\";\n")));
        assert!(!imports_express(&parsed("import express from \"express-rate-limit\";\n")));
        assert!(!imports_express(&parsed("const app = createServer();\n")));
    }

    /// The three shapes the argument list is read for, and the composition of
    /// two of them. A name that is not in the list is not a middleware however
    /// it is spelled.
    #[test]
    fn every_shape_of_a_named_middleware_is_read() {
        let names = vec!["requireAuth".to_string()];
        let authed = |source: &str| {
            let file = parsed(source);
            let regs = registrations(&file);
            let mounts = auth_mounts(&file, &names);
            is_authenticated(&regs[0], &file.source, &names, &mounts)
        };
        assert!(authed("app.get(\"/a\", requireAuth, h);\n"), "a bare identifier");
        assert!(authed("app.get(\"/a\", requireAuth(), h);\n"), "a call that builds one");
        assert!(authed("app.get(\"/a\", requireAuth(\"admin\"), h);\n"), "with arguments");
        assert!(authed("app.get(\"/a\", auth.requireAuth, h);\n"), "a member");
        assert!(authed("app.get(\"/a\", auth.requireAuth(), h);\n"), "a call on a member");
        assert!(authed("app.get(\"/a\", (requireAuth as Handler), h);\n"), "through a cast");
        // Express's own documented middleware-chain shape: one argument holding
        // the list. Any element authenticates it.
        assert!(authed("app.post(\"/orders\", [requireAuth, validate], create);\n"), "an array of middlewares");
        assert!(authed("app.post(\"/orders\", [validate, auth.requireAuth()], create);\n"), "any element, any shape");
        assert!(!authed("app.post(\"/orders\", [validate, log], create);\n"), "an array of other middlewares");
        assert!(!authed("app.get(\"/a\", requireAuthentication, h);\n"), "a longer name is another name");
        assert!(!authed("app.get(\"/a\", h);\n"));
        // The path is the first argument and is never itself a middleware, even
        // where the repository has named a middleware after it.
        let names = vec!["/a".to_string()];
        let file = parsed("app.get(\"/a\", h);\n");
        assert!(!is_authenticated(&registrations(&file)[0], &file.source, &names, &[]));
    }

    /// A registration is a member call on a plain identifier; anything else is
    /// somebody's other API with the same method name.
    #[test]
    fn a_registration_needs_an_identifier_receiver_and_a_known_method() {
        let file = parsed(
            "app.get(\"/a\", h);\nthis.app.get(\"/b\", h);\ngetApp().post(\"/c\", h);\nmap.set(\"/d\", h);\nrouter.use(mw);\n",
        );
        let regs = registrations(&file);
        let read: Vec<(String, Option<String>, u32)> =
            regs.iter().map(|r| (r.method.clone(), r.path.clone(), r.line)).collect();
        assert_eq!(read, vec![("get".to_string(), Some("/a".to_string()), 1), ("use".to_string(), None, 5),]);
    }

    /// A mount with no path covers everything after it, a mount with one covers
    /// what its path prefixes, and neither covers what came before.
    #[test]
    fn a_mount_covers_what_follows_it_and_only_what_it_prefixes() {
        let names = vec!["requireAuth".to_string()];
        let file =
            parsed("app.get(\"/early\", h);\napp.use(\"/admin\", requireAuth);\napp.get(\"/admin/users\", h);\napp.get(\"/jobs\", h);\n");
        let mounts = auth_mounts(&file, &names);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].prefix, "/admin");
        let covered: Vec<(String, bool)> = registrations(&file)
            .iter()
            .filter(|r| r.method != "use")
            .map(|r| (r.path.clone().unwrap_or_default(), is_authenticated(r, &file.source, &names, &mounts)))
            .collect();
        assert_eq!(
            covered,
            vec![("/early".to_string(), false), ("/admin/users".to_string(), true), ("/jobs".to_string(), false)]
        );

        let file = parsed("app.use(requireAuth);\napp.get(\"/anything\", h);\n");
        let mounts = auth_mounts(&file, &names);
        assert_eq!(mounts[0].prefix, "", "a mount with no path covers every path");
        let regs = registrations(&file);
        let route = regs.iter().find(|r| r.method == "get").unwrap();
        assert!(is_authenticated(route, &file.source, &names, &mounts));
    }

    /// A prefix covers whole segments, not characters. `/apiary` is not under
    /// `/api`, and a mount at `/api` still covers `/api` itself.
    #[test]
    fn a_prefix_is_matched_a_segment_at_a_time() {
        assert!(covers("/api", "/api"), "the mount point itself");
        assert!(covers("/api", "/api/users"));
        assert!(covers("/api", "/api/v1/users"));
        assert!(!covers("/api", "/apiary"), "the same characters are not the same route");
        assert!(!covers("/api", "/apikeys"));
        assert!(!covers("/api", "/other"));
        assert!(covers("", "/anything"), "a mount with no path covers everything");
        assert!(covers("/", "/anything"), "and so does one mounted at the root");
        assert!(covers("/api/", "/api/users"), "a trailing slash on the prefix changes nothing");

        let names = vec!["requireAuth".to_string()];
        let file = parsed("app.use(\"/api\", requireAuth);\napp.get(\"/apiary/bees\", h);\napp.get(\"/api\", h);\n");
        let mounts = auth_mounts(&file, &names);
        let covered: Vec<(String, bool)> = registrations(&file)
            .iter()
            .filter(|r| r.method != "use")
            .map(|r| (r.path.clone().unwrap_or_default(), is_authenticated(r, &file.source, &names, &mounts)))
            .collect();
        assert_eq!(covered, vec![("/apiary/bees".to_string(), false), ("/api".to_string(), true)]);
    }

    #[test]
    fn an_object_property_is_read_quoted_or_bare() {
        let file =
            parsed("const a = { origin: \"*\" };\nconst b = { \"origin\": true };\nconst c = { credentials: true };\n");
        let mut objects: Vec<Node> = Vec::new();
        walk(file.tree.root_node(), &mut |n: Node| {
            if n.kind() == "object" {
                objects.push(n);
            }
        });
        assert_eq!(objects.len(), 3);
        assert_eq!(property(objects[0], &file.source, "origin").map(|v| text(v, &file.source)), Some("\"*\""));
        assert_eq!(property(objects[1], &file.source, "origin").map(|v| text(v, &file.source)), Some("true"));
        assert!(property(objects[2], &file.source, "origin").is_none());
    }
}
