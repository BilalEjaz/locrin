//! Flags three uses of cryptography that are broken for the job the
//! surrounding code gives them: a broken hash over a credential, a
//! non-cryptographic random source producing a secret, and a fixed
//! initialisation vector.
//!
//! All three read one file's syntax tree and nothing else, which is where the
//! blind spots are:
//!
//! - **A weak hash is only a finding because of what it hashes.** MD5 and SHA-1
//!   over a cache key are a performance choice; over a password they are the
//!   whole vulnerability. Nothing in a syntax tree says which, so the rule reads
//!   the names around the call: the enclosing top-level symbol and every
//!   identifier in the enclosing statement. A credential-shaped name is High
//!   confidence, anything else is Medium and still reported, because a weak
//!   digest is worth a look even when the reviewer decides it is a cache key.
//!   The cost is that `createHash("md5")` over a variable named `input` inside a
//!   function named `hash` reads as Medium however sensitive `input` really is.
//! - **`Math.random()` is flagged by the name of what it fills, never by the
//!   call.** The call itself is unremarkable: jitter, a shuffle, a sampled log
//!   line, and a placeholder colour all use it correctly, and they are the
//!   overwhelming majority of its uses in an application. So the rule fires only
//!   when the value lands somewhere named like a secret (a token, a nonce, a
//!   salt, an IV, an OTP, an API key) or like an identifier that is expected to
//!   be unguessable. That is a deliberate under-report: a secret assigned to a
//!   name that does not say so is invisible here.
//! - **A static IV is read off the argument, not off the value.** A string
//!   literal, `Buffer.from` of a literal, `Buffer.alloc(n)` (an all-zero IV) and
//!   an array literal are all fixed bytes at the call site. An IV read from a
//!   constant declared three lines up, or from a config file, is the same bug
//!   and is not seen: resolving it needs the constant's value, which is release
//!   two's cross-function work. `crypto.randomBytes(16)` and any other
//!   expression are left alone.
//! - **No import resolution anywhere.** `createHash` is matched by name, bare or
//!   as a property, so a project-local helper that happens to be called
//!   `createHash` is judged as Node's. Requiring the import would silence the
//!   rule in every file that reaches crypto through a wrapper, which is most of
//!   them.

use std::sync::OnceLock;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use regex::Regex;
use tree_sitter::Node;

use crate::{anchor_for, clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct WeakCrypto;

const HASH_FIX: &str = "Use bcrypt, scrypt, or argon2 for credentials; SHA-256 or better for integrity";
const RANDOM_FIX: &str = "Use crypto.randomBytes, crypto.randomUUID, or crypto.getRandomValues";
const IV_FIX: &str =
    "Generate a fresh random IV per message with crypto.randomBytes and store it beside the ciphertext";

/// A name that says the value beside it is a credential, which is what turns a
/// weak digest from a performance choice into a vulnerability.
const CREDENTIAL: &str = r"(?i)(password|passwd|pwd|credential|secret|token|session)";
/// A name that says the value being filled must be unguessable. Every part is
/// required to stand as its own word in the identifier; see `boundary`.
const SECRET_NAME: &str = r"(?i)(token|secret|nonce|session|otp|password|salt|iv|api[_-]?key)";
/// A name that says the value is a universally unique identifier, which
/// `Math.random` cannot make one of.
///
/// The plan wrote this as `id$|uuid|guid` with a six-character minimum. The
/// `id$` half is gone and the minimum with it, because on the corpus it found
/// three names and all three were domain keys rather than anything an attacker
/// would guess: `exerciseId` in a unit-test factory, and `tempId` twice, an
/// optimistic React list key for a photo being uploaded. A name ending in `Id`
/// is how application code spells "the key of a row", so the half was a class
/// of false positives rather than a source of findings, and the names that end
/// in `Id` *and* mean a secret (`sessionId`, `tokenId`) are already
/// `SECRET_NAME`. Restoring it is a one-line change if a repository disagrees.
const ID_NAME: &str = r"(?i)(uuid|guid)";

fn credential() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(CREDENTIAL).expect("the credential pattern compiles"))
}

fn secret_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(SECRET_NAME).expect("the secret name pattern compiles"))
}

fn id_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(ID_NAME).expect("the id name pattern compiles"))
}

/// Whether a `SECRET_NAME` match stands as its own word inside the identifier:
/// it begins the name, follows a separator, or begins a camel-case part, and it
/// ends the name or runs up against one of those. A bare substring test reads
/// the `iv` in `private`, `derive` and `activity` as an initialisation vector;
/// on the corpus that is exactly what it did, flagging a test helper named
/// `sellerWithActiveListing` as a generator of IVs.
fn boundary(name: &str, start: usize, end: usize) -> bool {
    let b = name.as_bytes();
    let starts_word = start == 0 || !b[start - 1].is_ascii_alphanumeric() || b[start].is_ascii_uppercase();
    let ends_word =
        end == b.len() || !b[end].is_ascii_alphanumeric() || b[end].is_ascii_uppercase() || b[end].is_ascii_digit();
    starts_word && ends_word
}

/// Whether `name` names a secret: a `SECRET_NAME` part standing as its own word.
fn names_a_secret(name: &str) -> bool {
    secret_name().find_iter(name).any(|m| boundary(name, m.start(), m.end()))
}

/// Whether `name` names an identifier that has to be unguessable.
fn names_an_id(name: &str) -> bool {
    id_name().is_match(name)
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
    let Some(list) = call.child_by_field_name("arguments") else { return vec![] };
    let mut cursor = list.walk();
    list.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect()
}

/// The name a call reaches, bare (`createHash(...)`) or as a property
/// (`crypto.createHash(...)`). Anything else, including a call through an
/// index, is not a name this rule can read.
fn callee_name<'a>(call: Node<'a>, src: &'a str) -> Option<&'a str> {
    let f = call.child_by_field_name("function")?;
    match f.kind() {
        "identifier" => Some(text(f, src)),
        "member_expression" => Some(text(f.child_by_field_name("property")?, src)),
        _ => None,
    }
}

/// Whether the call is `<object>.<property>(...)` with those exact names.
fn is_member_call(call: Node, src: &str, object: &str, property: &str) -> bool {
    let Some(f) = call.child_by_field_name("function").filter(|f| f.kind() == "member_expression") else {
        return false;
    };
    f.child_by_field_name("object").is_some_and(|o| text(o, src) == object)
        && f.child_by_field_name("property").is_some_and(|p| text(p, src) == property)
}

/// The content of a string literal, or `None` when the node is not one. A
/// template string counts only when nothing is interpolated into it, because an
/// interpolated one is not a fixed value.
fn literal_string<'a>(node: Node<'a>, src: &'a str) -> Option<String> {
    let mut cursor = node.walk();
    match node.kind() {
        "string" => Some(
            node.named_children(&mut cursor).filter(|c| c.kind() == "string_fragment").map(|c| text(c, src)).collect(),
        ),
        "template_string" => {
            let children: Vec<Node> = node.named_children(&mut cursor).collect();
            if children.iter().any(|c| c.kind() == "template_substitution") {
                return None;
            }
            Some(children.iter().filter(|c| c.kind() == "string_fragment").map(|c| text(*c, src)).collect())
        }
        _ => None,
    }
}

/// Every identifier in the statement the node sits in, which is the widest a
/// single expression's context gets without leaving the line's own decision.
fn statement_identifiers(node: Node, src: &str) -> Vec<String> {
    let mut current = Some(node);
    let mut stmt = node;
    while let Some(n) = current {
        if n.kind().ends_with("_statement") || n.kind() == "variable_declarator" {
            stmt = n;
            break;
        }
        current = n.parent();
    }
    let mut out = Vec::new();
    walk(stmt, &mut |n: Node| {
        if matches!(n.kind(), "identifier" | "property_identifier" | "shorthand_property_identifier") {
            out.push(text(n, src).to_string());
        }
    });
    out
}

/// The name of the nearest enclosing function-like declaration. `enclosing_symbol`
/// answers for the top-level symbol; this answers for a nested helper, which is
/// where a generator of secrets usually lives.
fn enclosing_function_name(node: Node, src: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(n) = current {
        let name = match n.kind() {
            "function_declaration" | "generator_function_declaration" | "method_definition" => {
                n.child_by_field_name("name")
            }
            "variable_declarator" => n
                .child_by_field_name("value")
                .filter(|v| matches!(v.kind(), "arrow_function" | "function_expression"))
                .and_then(|_| n.child_by_field_name("name")),
            _ => None,
        };
        if let Some(name) = name {
            return Some(text(name, src).to_string());
        }
        current = n.parent();
    }
    None
}

/// What the expression's value is being called, as far as the enclosing
/// statement says: the name it is bound to, the name it is assigned to, or the
/// key it is stored under. A member target answers with its property, which is
/// the part that carries the meaning (`this.sessionToken`).
fn assignment_target(node: Node, src: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(n) = current {
        let target = match n.kind() {
            "variable_declarator" | "public_field_definition" => n.child_by_field_name("name"),
            "assignment_expression" => n.child_by_field_name("left"),
            "pair" => n.child_by_field_name("key"),
            _ => None,
        };
        if let Some(t) = target {
            return match t.kind() {
                "identifier" | "property_identifier" => Some(text(t, src).to_string()),
                "member_expression" => t.child_by_field_name("property").map(|p| text(p, src).to_string()),
                "string" => literal_string(t, src),
                _ => None,
            };
        }
        if n.kind().ends_with("_statement") {
            return None;
        }
        current = n.parent();
    }
    None
}

/// Whether the third argument of a cipher call is a fixed value written at the
/// call site. See the module doc for what this cannot see.
fn static_iv(node: Node, src: &str) -> bool {
    match node.kind() {
        "string" | "array" => true,
        "template_string" => literal_string(node, src).is_some(),
        "call_expression" => {
            let first = args(node).into_iter().next();
            if is_member_call(node, src, "Buffer", "from") {
                return first.is_some_and(|a| matches!(a.kind(), "array") || literal_string(a, src).is_some());
            }
            if is_member_call(node, src, "Buffer", "alloc") {
                return first.is_some_and(|a| a.kind() == "number");
            }
            false
        }
        _ => false,
    }
}

/// The form a finding came from, which fixes its CWE and how it is spelled.
struct Form {
    evidence: String,
    fix: &'static str,
    cwe: &'static str,
    confidence: Confidence,
}

fn weak_hash(call: Node, src: &str, file: &ParsedFile, at: u32) -> Option<Form> {
    if callee_name(call, src)? != "createHash" {
        return None;
    }
    let alg = literal_string(*args(call).first()?, src)?;
    if !alg.eq_ignore_ascii_case("md5") && !alg.eq_ignore_ascii_case("sha1") {
        return None;
    }
    // The name that decides confidence is also the name the evidence prints, so
    // a High finding reads as the credential it found and a Medium one as the
    // symbol it could not judge.
    let symbol = locrin_core::symbols::enclosing_symbol(file, at);
    // The identifier is preferred over the symbol because it is the thing being
    // hashed rather than the function doing it: `md5 used to hash password`
    // says more than `md5 used to hash hashPassword`.
    let credential = statement_identifiers(call, src)
        .into_iter()
        .find(|i| credential().is_match(i))
        .or_else(|| symbol.as_deref().filter(|s| credential().is_match(s)).map(|s| s.to_string()));
    let (context, confidence) = match credential {
        Some(name) => (name, Confidence::High),
        None => (symbol.unwrap_or_else(|| "a value".to_string()), Confidence::Medium),
    };
    Some(Form { evidence: format!("{alg} used to hash {context}"), fix: HASH_FIX, cwe: "CWE-327", confidence })
}

fn weak_random(call: Node, src: &str, file: &ParsedFile, at: u32) -> Option<Form> {
    if !is_member_call(call, src, "Math", "random") {
        return None;
    }
    // In order of how directly the name describes the value: what it is bound
    // to, the helper it is produced in, then the top-level symbol.
    let candidates = [
        assignment_target(call, src),
        enclosing_function_name(call, src),
        locrin_core::symbols::enclosing_symbol(file, at),
    ];
    let name = candidates.into_iter().flatten().find(|n| names_a_secret(n) || names_an_id(n))?;
    Some(Form {
        evidence: format!("Math.random() generates {name}"),
        fix: RANDOM_FIX,
        cwe: "CWE-338",
        confidence: Confidence::High,
    })
}

fn fixed_iv(call: Node, src: &str) -> Option<Form> {
    let callee = callee_name(call, src)?;
    if callee != "createCipheriv" && callee != "createDecipheriv" {
        return None;
    }
    if !static_iv(*args(call).get(2)?, src) {
        return None;
    }
    Some(Form {
        evidence: format!("static IV passed to {callee}"),
        fix: IV_FIX,
        cwe: "CWE-327",
        confidence: Confidence::High,
    })
}

fn scan(rule: &WeakCrypto, file: &ParsedFile) -> Vec<Finding> {
    let src = &file.source;
    let mut out: Vec<Finding> = Vec::new();
    walk(file.tree.root_node(), &mut |n: Node| {
        if n.kind() != "call_expression" {
            return;
        }
        let at = line(n);
        let Some(form) =
            weak_hash(n, src, file, at).or_else(|| weak_random(n, src, file, at)).or_else(|| fixed_iv(n, src))
        else {
            return;
        };
        // The evidence joins the symbol in the anchor so that two of these in
        // one function are two findings rather than one id written twice. Both
        // halves are names, so neither moves when a line above the call does.
        let anchor = format!("{}\x1f{}", anchor_for(file, at), form.evidence);
        let mut finding = finding_at(rule, &file.rel, line_span(file, at), &anchor, &form.evidence, form.fix);
        finding.confidence = form.confidence;
        finding.owasp = Some("A02:2021".to_string());
        finding.cwe = Some(form.cwe.to_string());
        out.push(finding);
    });
    out.sort_by_key(|f| (f.span.start_line, f.span.start_col));
    out
}

impl Rule for WeakCrypto {
    fn id(&self) -> &'static str {
        "weak-crypto"
    }
    fn description(&self) -> &'static str {
        "A broken hash over a credential, a guessable secret, or a fixed initialisation vector"
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
    /// High is the rule's own answer, which the random and IV forms keep: both
    /// read a fixed shape at the call site. The weak-hash form lowers a finding
    /// to Medium after construction when no name around the call says a
    /// credential is being hashed. See the module doc.
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{names_a_secret, names_an_id};

    /// The names the corpus run turned up, pinned so the tightening they forced
    /// cannot quietly come undone. See `boundary` and `ID_NAME`.
    #[test]
    fn a_secret_part_has_to_stand_as_its_own_word() {
        for yes in ["iv", "IV", "iv_bytes", "sessionIv", "authToken", "otp", "api_key", "apiKey", "salt"] {
            assert!(names_a_secret(yes), "{yes}");
        }
        for no in ["private", "privateKey", "derive", "driver", "activity", "sellerWithActiveListing", "archive"] {
            assert!(!names_a_secret(no), "{no}");
        }
    }

    #[test]
    fn a_domain_key_is_not_an_unguessable_identifier() {
        for yes in ["uuid", "newUuid", "requestGuid"] {
            assert!(names_an_id(yes), "{yes}");
        }
        for no in ["tempId", "exerciseId", "id", "listingId"] {
            assert!(!names_an_id(no), "{no}");
        }
    }
}
