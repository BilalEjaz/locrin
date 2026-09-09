//! Flags the Supabase service-role key where it can reach a client bundle.
//!
//! A Supabase project ships two keys that look identical. The anon key is meant
//! to be in the browser: every request it makes is filtered by the row-level
//! security policies on the table. The service-role key turns those policies
//! off, and it is meant to live on a server. Shipping it in a bundle publishes
//! the whole database, and the shipping is usually an accident: the key gets
//! pasted into a screen during a debugging session, or an environment variable
//! that was read in an edge function gets read in a component instead.
//!
//! What counts as "a client file" is the repository's answer, not this rule's.
//! `framework.server_paths` (spec 7.5) lists the globs whose files run on a
//! server; a file matching one of them is exempt, everything else is client
//! code. The defaults name the places server code usually lives
//! (`supabase/functions/**`, `server/**`, `api/**`, `scripts/**`,
//! `**/*.server.*`) and the two test globs, because a test that seeds rows
//! needs the key that bypasses the policies and never ships. A repository whose
//! server code is somewhere else says so in `locrin.toml`, and the list it
//! writes replaces the defaults rather than adding to them.
//!
//! Two shapes are read, and both are line shapes rather than syntax:
//!
//! - **The name.** Any line containing `service_role`, case-insensitively.
//!   `SUPABASE_SERVICE_ROLE_KEY` is how the environment variable is spelled
//!   everywhere, and `service_role` is the string the JWT payload carries, so
//!   the substring is the signal. The evidence widens the match out to the
//!   whole identifier it sits in, so the reader is told the name they will find
//!   on the line rather than the eight characters that matched.
//! - **The token.** A JWT literal whose decoded `role` claim is `service_role`.
//!   That is the case where the name is nowhere on the line: the key is pasted
//!   in as a bare string, and only the payload separates it from the anon key
//!   beside it. [`crate::secrets::jwt::role`] is the same decoder
//!   `secret-exposed` uses.
//!
//! A file that declares the name as a member of an `interface` or a `type` is
//! exempt from the name arm, because TypeScript has already said where the
//! value comes from: the runtime provides it, and the file writes it down
//! nowhere. That came out of the corpus. `fastlift-admin` is a Cloudflare
//! Worker whose whole source tree is server code and matches none of the
//! default `server_paths`, and all six findings the rule produced there were
//! the two shapes one `export interface Env { SUPABASE_SERVICE_ROLE_KEY:
//! string }` creates: the declaration line, and every `env.SUPABASE_SERVICE_
//! ROLE_KEY` read off a parameter of that type. None of it reaches a browser.
//! The token arm is not exempted with it, because a pasted key is an exposure
//! whatever the file's types say.
//!
//! Blind spots, all of them deliberate:
//!
//! - **A file that declares the binding goes quiet on the name.** A client file
//!   that declares `interface Env { SUPABASE_SERVICE_ROLE_KEY: string }` and
//!   then really does inline the key is reported only if the token itself is in
//!   it. The exemption is what the declaration is worth: a name reached through
//!   a runtime binding is not a name a bundler can inline.
//! - **`serviceRole` in camel case is not a match.** The rule matches the
//!   underscore spelling, which is the one Supabase itself uses in the payload
//!   and in every environment variable it documents. Matching `service` next to
//!   `role` in any casing would pull in `serviceRoleId` on an internal
//!   permissions model, which is a different concept with the same words. A
//!   line that reads the key almost always names the environment variable on
//!   it, and the JWT arm catches the case where it does not.
//! - **A line that is only a comment is skipped.** A comment saying which key
//!   belongs where is documentation, and a rule that reports it teaches the
//!   repository to stop writing the documentation. Code with a trailing comment
//!   is still code, so only a line whose first non-space characters open a
//!   comment is skipped.
//! - **There is no dataflow here.** The rule says the name or the token is on
//!   the line, not that the value reaches a request. A file that names the
//!   variable to assert it is absent is a finding, and `locrin:allow` is the
//!   answer to it.
//! - **An expired token still counts.** `secret-exposed` discards a token whose
//!   `exp` has passed, because there is nothing left to revoke. This rule does
//!   not: a service-role key in client code is a mistake about where code runs,
//!   and the fix is the same whether the pasted token is live or stale.

use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::text;
use regex::Regex;
use tree_sitter::Node;

use crate::secrets::jwt;
use crate::{clean_files, finding, Rule, RuleContext, Scope};

pub struct SupabaseServiceRoleInClient;

/// The role claim that turns row-level security off.
const SERVICE_ROLE: &str = "service_role";

const FIX: &str = "Move this call behind a server (edge function, API route) and use the anon key on the client; the service role bypasses row-level security";

/// The longest line the rule looks at, matching `secret-exposed`. One enormous
/// line is a generated bundle or an inlined asset, and running two regexes over
/// a megabyte of it on every file costs more than the miss.
const MAX_LINE: usize = 2000;

/// The name, case-insensitively, with no word boundaries: the boundary before
/// `service` would never fire inside `SUPABASE_SERVICE_ROLE_KEY`, because an
/// underscore is a word character.
fn name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)service_role").unwrap())
}

/// The shape of a JWT, the same one `secrets::patterns` matches: three
/// base64url segments, the first two of which start `eyJ` because both are
/// JSON objects.
fn jwt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b").unwrap())
}

/// The `server_paths` globs, compiled. An unparseable glob is dropped rather
/// than failing the run: `Config::load` has already rejected the bad pattern,
/// so anything reaching here came from a caller that built its own config.
fn server_set(globs: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        if let Ok(glob) = Glob::new(g) {
            b.add(glob);
        }
    }
    b.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap())
}

/// Whether the line's first non-space characters open a comment. Code with a
/// comment after it is code.
fn comment_only(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// The whole identifier the match sits inside, so the evidence names
/// `SUPABASE_SERVICE_ROLE_KEY` rather than the `SERVICE_ROLE` in the middle of
/// it. Identifier characters are letters, digits, underscores and dollars,
/// which covers an environment variable, a TypeScript name and a JSON key
/// alike.
fn identifier_around(line: &str, start: usize, end: usize) -> String {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut from = start;
    while let Some(prev) = line[..from].chars().next_back() {
        if !ident(prev) {
            break;
        }
        from -= prev.len_utf8();
    }
    let mut to = end;
    while let Some(next) = line[to..].chars().next() {
        if !ident(next) {
            break;
        }
        to += next.len_utf8();
    }
    line[from..to].to_string()
}

/// Whether the file declares the name as a member of an `interface` or a
/// `type`, which is TypeScript's own way of saying the value arrives from the
/// runtime rather than being written down here.
///
/// This is the corpus tightening. See the module doc: on `fastlift-admin` every
/// finding the rule produced was one of the two shapes such a declaration
/// creates, the `SUPABASE_SERVICE_ROLE_KEY: string` line inside
/// `export interface Env` and the `env.SUPABASE_SERVICE_ROLE_KEY` reads off a
/// parameter of that type, and both live in a file that ships to no browser.
///
/// `property_signature` is the TypeScript grammar's node for a member of an
/// interface body or an object type, and nothing else, so the check is exact
/// rather than a guess at the shape of a line.
fn declares_the_binding(file: &ParsedFile) -> bool {
    fn walk(node: Node, src: &str) -> bool {
        if node.kind() == "property_signature" {
            let name = node.child_by_field_name("name").map(|n| text(n, src)).unwrap_or("");
            if name_re().is_match(name) {
                return true;
            }
        }
        // Two statements, not one expression: the iterator borrows `cursor`, so
        // it has to be dropped before `cursor` is.
        let mut cursor = node.walk();
        let found = node.children(&mut cursor).any(|c| walk(c, src));
        found
    }
    walk(file.tree.root_node(), &file.source)
}

/// What the line holds, as the evidence spells it, or `None` when it holds
/// nothing. The name is asked first because it costs one regex over a short
/// line; the JWT arm decodes a payload and only runs when the name is absent.
///
/// `names` is false in a file that declares the binding: the name arm is off
/// there and the token arm is not, because a pasted key is an exposure whatever
/// the file's types say.
///
/// One answer per line, so a line naming the variable twice is one finding.
fn what(line: &str, names: bool) -> Option<String> {
    if line.len() > MAX_LINE || comment_only(line) {
        return None;
    }
    if names {
        if let Some(m) = name_re().find(line) {
            return Some(identifier_around(line, m.start(), m.end()));
        }
    }
    for m in jwt_re().find_iter(line) {
        if jwt::role(m.as_str()).as_deref() == Some(SERVICE_ROLE) {
            return Some(format!("a {SERVICE_ROLE} JWT"));
        }
    }
    None
}

fn scan(rule: &SupabaseServiceRoleInClient, file: &ParsedFile) -> Vec<Finding> {
    let names = !declares_the_binding(file);
    let mut out = Vec::new();
    for (i, line) in file.source.lines().enumerate() {
        let Some(what) = what(line, names) else { continue };
        // The regexes run over the untrimmed line, because a comment is
        // recognised by what it starts with and a JWT can sit at either end.
        let number = i as u32 + 1;
        let mut f = finding(rule, file, number, &format!("service-role key referenced in client code ({what})"), FIX);
        f.owasp = Some("A01:2021".to_string());
        f.cwe = Some("CWE-284".to_string());
        out.push(f);
    }
    out
}

impl Rule for SupabaseServiceRoleInClient {
    fn id(&self) -> &'static str {
        "supabase-service-role-in-client"
    }
    fn description(&self) -> &'static str {
        "Supabase service-role key referenced outside server code"
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
        let server = server_set(&ctx.config.framework.server_paths);
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            if server.is_match(&file.rel) {
                continue;
            }
            out.extend(scan(self, file));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
    const ANON: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYW5vbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";

    /// [`what`] in a file that does not declare the binding, which is what
    /// every case below is about; the declaring file has its own test.
    fn what2(line: &str) -> Option<String> {
        what(line, true)
    }

    fn parsed(source: &str) -> ParsedFile {
        locrin_core::parse::parse_source(std::path::Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    /// The corpus tightening. Both shapes `fastlift-admin` produced are here:
    /// the declaration itself and the read off a value of that type.
    #[test]
    fn a_file_that_declares_the_binding_is_reading_it_from_the_runtime() {
        let worker = parsed(
            "export interface Env {\n  SUPABASE_URL: string;\n  SUPABASE_SERVICE_ROLE_KEY: string;\n}\n\nexport function headers(env: Env) {\n  return { apikey: env.SUPABASE_SERVICE_ROLE_KEY };\n}\n",
        );
        assert!(declares_the_binding(&worker));

        // An object type says the same thing as an interface.
        assert!(declares_the_binding(&parsed("type Env = { SUPABASE_SERVICE_ROLE_KEY: string };\n")));

        // A file that only reads the name declares nothing, and is still read.
        let reader = parsed("const k = process.env.SUPABASE_SERVICE_ROLE_KEY;\n");
        assert!(!declares_the_binding(&reader));

        // An interface that declares some other member is not a declaration of
        // this one.
        assert!(!declares_the_binding(&parsed("interface Env {\n  SUPABASE_ANON_KEY: string;\n}\n")));
    }

    /// The declaration turns off the name arm and not the token arm: a pasted
    /// key is an exposure whatever the file's types say.
    #[test]
    fn the_token_arm_survives_the_declaration() {
        assert_eq!(what("  SUPABASE_SERVICE_ROLE_KEY: string;", false), None);
        assert_eq!(what("  return env.SUPABASE_SERVICE_ROLE_KEY;", false), None);
        assert_eq!(what(&format!("const k = \"{SERVICE}\";"), false).as_deref(), Some("a service_role JWT"));
    }

    #[test]
    fn the_name_is_read_out_to_the_whole_identifier_in_any_casing() {
        assert_eq!(
            what2("const k = process.env.SUPABASE_SERVICE_ROLE_KEY;").as_deref(),
            Some("SUPABASE_SERVICE_ROLE_KEY")
        );
        assert_eq!(what2("  service_role: true,").as_deref(), Some("service_role"));
        assert_eq!(what2("Deno.env.get(\"SERVICE_ROLE\")").as_deref(), Some("SERVICE_ROLE"));
        // Twice on one line is still one answer, which is what makes the rule
        // one finding per line.
        assert_eq!(
            what2("createClient(env.SERVICE_ROLE_URL, env.SERVICE_ROLE_KEY)").as_deref(),
            Some("SERVICE_ROLE_URL")
        );
        // The underscore spelling only. See the module doc.
        assert_eq!(what2("const serviceRoleKey = k;"), None);
    }

    #[test]
    fn a_token_is_read_by_its_payload_and_the_anon_key_is_not_a_finding() {
        assert_eq!(what2(&format!("const k = \"{SERVICE}\";")).as_deref(), Some("a service_role JWT"));
        assert_eq!(what2(&format!("const k = \"{ANON}\";")), None);
        assert_eq!(what2("const k = \"eyJnot.a.token\";"), None);
    }

    #[test]
    fn a_line_that_is_only_a_comment_is_documentation() {
        assert_eq!(what2("// the service_role key lives in the edge function"), None);
        assert_eq!(what2("  * SUPABASE_SERVICE_ROLE_KEY is read there, not here"), None);
        assert_eq!(what2("/* service_role */"), None);
        // A comment after code does not make the code a comment.
        assert_eq!(
            what2("const k = env.SUPABASE_SERVICE_ROLE_KEY; // server only").as_deref(),
            Some("SUPABASE_SERVICE_ROLE_KEY")
        );
    }

    #[test]
    fn one_enormous_line_is_a_bundle_and_is_not_read() {
        let long = format!("{}SUPABASE_SERVICE_ROLE_KEY", "x".repeat(MAX_LINE));
        assert_eq!(what2(&long), None);
    }

    /// An empty list exempts nothing, which is what a repository asking for
    /// every file to be checked writes.
    #[test]
    fn an_empty_server_paths_list_exempts_nothing() {
        let set = server_set(&[]);
        assert!(!set.is_match("supabase/functions/x/index.ts"));
        let set = server_set(&["supabase/functions/**".to_string(), "not a [ glob".to_string()]);
        assert!(set.is_match("supabase/functions/x/index.ts"), "the good glob survives the bad one");
    }
}
