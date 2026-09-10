//! Flags three families of sink where a value that was built rather than
//! written reaches an interpreter: a code evaluator, a shell, and a SQL driver.
//!
//! The rule reads one file's syntax tree, so it sees the shape of the argument
//! at the call and never where the value came from. That is the whole of its
//! judgement, and it is where the blind spots are:
//!
//! - **Nothing here is taint tracking.** A template with a substitution in it
//!   reaching `db.query` is a finding whether the substitution is a request
//!   parameter or a constant declared two modules away. The reverse is worse: a
//!   query assembled in a helper and passed in as a parameter is invisible,
//!   because the shape at the call is a plain identifier from another function.
//!   Cross-function flow is release two's work; this rule reports the shape.
//! - **A bare identifier is resolved exactly one step, in the same file.** For a
//!   SQL sink the rule looks for the identifier's own `const`/`let` declaration
//!   and reads its value: a template with substitutions or a concatenation is
//!   the same finding as writing it inline, so it is High. Anything else is
//!   Medium, which is the honest answer for `db.query(q)` where `q` came from a
//!   parameter. It does not chase the declaration's own operands, and it does
//!   not follow reassignment, so a query built in two steps reads as the first.
//! - **Callee names are matched, imports are not resolved.** `exec` and
//!   `execSync` are a shell bare, or under an object that names the
//!   child-process module (`child_process`, `cp`, `shell`, `sh`). Under any
//!   other object the call is one of three things and the argument decides:
//!   `RegExp.prototype.exec` (a regular-expression receiver, read at the call
//!   or resolved one step, and never a sink), SQL when the string carries a
//!   query word or when this file cannot read the string at all, and a shell
//!   command otherwise. So `runner.exec(`rm -rf ${path}`)` is a command sink
//!   rather than a query, and `db.exec(`insert into ...`)` and `db.exec(stmt)`
//!   stay SQL. A wrapper whose command line this file cannot read is still
//!   filed as SQL, which is the commoner meaning of an unplaceable `exec`.
//! - **SQL inside a test file is advisory.** A test that seeds a fixture
//!   database by interpolating a constant into `INSERT INTO` is doing exactly
//!   what the rule says and nothing a reviewer would change: on the corpus that
//!   shape was 25 of 28 findings, every one accurate and every one harmless. A
//!   SQL-family finding in a file `locrin_core::testcases::is_test_file`
//!   recognises therefore stands at Medium confidence whatever its shape. The
//!   evidence, the severity and the CWE are unchanged, and the code and command
//!   families are untouched: a shell command built in a test script runs on the
//!   same machine as one built anywhere else.
//! - **A constant is not a finding, however it is spelled.** `"ls " + "-la"` and
//!   a template with no substitution are fixed strings, so they are literals
//!   here even though the syntax is an expression. Only a non-literal operand
//!   makes a concatenation a finding.
//! - **A tagged template is safe by construction and is skipped.** `` sql`...` ``
//!   parses as a call whose `arguments` field is the template itself rather than
//!   an argument list, and a tag that parameterises is the fix this rule
//!   recommends. A tag that concatenates instead is therefore invisible.
//! - **`setTimeout` asks a question the others do not.** Its first argument is
//!   almost always a function, so a bare identifier there is only a finding when
//!   the same file declares it as a string. `eval` and `new Function` take no
//!   such care: their argument is code whatever its type, so any non-literal is
//!   High.

use std::collections::HashMap;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{line, text};
use tree_sitter::Node;

use crate::{anchor_for, clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct InjectionSink;

const CODE_FIX: &str = "Do not build code from data; use a lookup table or JSON.parse";
const COMMAND_FIX: &str =
    "Pass arguments as an array to execFile or spawn without a shell, and validate or allow-list every piece";
const SQL_FIX: &str =
    "Use parameter placeholders ($1, ?) or a tagged template that parameterises, and pass values separately";

/// Callees that hand their first argument to a shell, bare or as a property of
/// the child-process module. See the module doc for why the object is checked.
const SHELL_CALLEES: [&str; 2] = ["exec", "execSync"];
/// Objects a shell callee may hang off. Anything else is another `exec`.
const SHELL_OBJECTS: [&str; 5] = ["child_process", "childProcess", "cp", "shell", "sh"];
/// Callees that take an argv array and reach a shell only when told to.
const ARGV_CALLEES: [&str; 3] = ["spawn", "spawnSync", "execFile"];
/// Properties that run their first argument as SQL. `$queryRaw` and
/// `$executeRaw` are absent on purpose: they parameterise their template.
const SQL_PROPERTIES: [&str; 7] = ["query", "raw", "execute", "exec", "$queryRawUnsafe", "$executeRawUnsafe", "unsafe"];
/// The words that make a string a query rather than a command line, used to
/// tell the two meanings of `exec` apart when the receiver says neither. See
/// `carries_sql`.
/// `with` is here because a common table expression is how a non-trivial
/// `select` or `delete` is written, and the statement then opens with the `with`
/// rather than with the verb: `with recent as (...) delete from sessions where
/// id in (select id from recent)` is SQL by any reading and opened with a word
/// this list did not hold. `merge`, `explain`, `grant` and `revoke` are the
/// other statement openers a repository writes.
///
/// `set` is not here. It opens a statement in both languages, and a shell script
/// opens with `set -e` far more often than a query opens with `set`; see
/// [`sets_a_database_setting`].
const SQL_KEYWORDS: [&str; 19] = [
    "select", "insert", "update", "delete", "create", "drop", "alter", "replace", "pragma", "attach", "begin",
    "commit", "truncate", "vacuum", "with", "merge", "explain", "grant", "revoke",
];

/// The words a database `set` names when it is not assigning a setting by name:
/// `set search_path to ...`, `set session ...`, `set local ...`, `set role ...`,
/// `set transaction ...`. See [`sets_a_database_setting`].
const SQL_SETTINGS: [&str; 5] = ["search_path", "session", "local", "role", "transaction"];

/// How the argument was built, which is all the evidence a syntax tree offers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Variable,
    Template,
    Concatenation,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Variable => "a variable",
            Kind::Template => "a template with substitutions",
            Kind::Concatenation => "a concatenation",
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

/// The named arguments of a call or a `new`, comments dropped. A tagged
/// template has a `template_string` where the argument list would be, and it
/// answers with nothing: it has no argument list to read.
fn args<'a>(call: Node<'a>) -> Vec<Node<'a>> {
    let Some(list) = call.child_by_field_name("arguments").filter(|a| a.kind() == "arguments") else {
        return vec![];
    };
    let mut cursor = list.walk();
    list.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect()
}

/// Whether the call is a tagged template (`` sql`...` ``), which parameterises
/// rather than concatenates and is the fix this rule recommends.
fn is_tagged_template(call: Node) -> bool {
    call.child_by_field_name("arguments").is_some_and(|a| a.kind() == "template_string")
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

/// Whether the node is a fixed value written here: a string, a number, a
/// template with nothing interpolated, or an addition of those. A constant
/// spelled as an expression is still a constant, so `"ls " + "-la"` is a
/// literal and not a finding.
fn is_literal(node: Node, src: &str) -> bool {
    let node = unwrap(node);
    match node.kind() {
        "string" | "number" => true,
        "template_string" => !has_substitution(node),
        _ if is_addition(node, src) => {
            let left = node.child_by_field_name("left");
            let right = node.child_by_field_name("right");
            match (left, right) {
                (Some(l), Some(r)) => is_literal(l, src) && is_literal(r, src),
                _ => false,
            }
        }
        _ => false,
    }
}

fn has_substitution(node: Node) -> bool {
    // The answer is bound before it is returned: the iterator borrows `cursor`,
    // so it has to be dropped before `cursor` is.
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|c| c.kind() == "template_substitution");
    found
}

/// The kind of a non-literal argument at a command or SQL sink: the three
/// shapes the brief names, and nothing else. A member expression or a call
/// reaching a sink is a real risk, but it is one the shape cannot judge, so it
/// is left to the cross-function work rather than reported at a guess.
fn sink_kind(node: Node, src: &str) -> Option<Kind> {
    let node = unwrap(node);
    if is_literal(node, src) {
        return None;
    }
    match node.kind() {
        "template_string" => Some(Kind::Template),
        "identifier" => Some(Kind::Variable),
        _ if is_addition(node, src) => Some(Kind::Concatenation),
        _ => None,
    }
}

/// The kind of a non-literal argument at a code sink, where anything that is
/// not a fixed string is code built from data.
fn code_kind(node: Node, src: &str) -> Option<Kind> {
    let node = unwrap(node);
    if is_literal(node, src) {
        return None;
    }
    match node.kind() {
        "template_string" => Some(Kind::Template),
        _ if is_addition(node, src) => Some(Kind::Concatenation),
        _ => Some(Kind::Variable),
    }
}

/// Every `const`/`let`/`var` declaration in the file, by name. The first
/// declaration of a name wins: a name declared twice in one file is two
/// different values in two scopes, and picking either is a guess, so the rule
/// picks the one a reader meets first.
fn declarations<'a>(root: Node<'a>, src: &'a str) -> HashMap<&'a str, Node<'a>> {
    let mut out: HashMap<&str, Node> = HashMap::new();
    walk(root, &mut |n: Node<'a>| {
        if n.kind() != "variable_declarator" {
            return;
        }
        let (Some(name), Some(value)) = (n.child_by_field_name("name"), n.child_by_field_name("value")) else {
            return;
        };
        if name.kind() == "identifier" {
            out.entry(text(name, src)).or_insert(value);
        }
    });
    out
}

/// Whether a declared value was itself built from data, which is what turns a
/// bare identifier at a SQL sink from Medium into High.
fn built_from_data(value: Node, src: &str) -> bool {
    matches!(sink_kind(value, src), Some(Kind::Template | Kind::Concatenation))
}

/// The fixed text an argument carries: the fragments of a template, the string
/// literals of a concatenation. Empty when the shape carries none, which is the
/// answer for a bare identifier.
fn literal_text(node: Node, src: &str) -> String {
    let node = unwrap(node);
    let mut out = String::new();
    walk(node, &mut |n: Node| {
        if n.kind() == "string_fragment" {
            out.push_str(text(n, src));
            out.push(' ');
        }
    });
    out
}

/// Whether the text an argument carries reads as SQL. A value this file cannot
/// read carries no text and answers yes: `exec` on an object that is neither a
/// child-process module nor a regular expression is a database handle far more
/// often than it is a shell wrapper, so SQL is the answer to fall back on. See
/// the module doc.
///
/// Only the first word is asked, with one exception. A statement is named by the
/// verb it opens with, and a command line is too: `kubectl delete pod ${name}`
/// and `git update-index --refresh` are commands whose second word happens to be
/// a SQL keyword, and asking the whole string filed both under `CWE-89` with a
/// fix about parameter placeholders. The first word answers `select ... where
/// id = ${id}` exactly as well and answers those correctly too.
///
/// The exception is `set`, which opens a statement in both languages. See
/// [`sets_a_database_setting`] for what decides it.
fn carries_sql(node: Node, src: &str) -> bool {
    let carried = literal_text(node, src).to_ascii_lowercase();
    let mut words = carried.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty());
    match words.next() {
        None => true,
        Some("set") => sets_a_database_setting(&carried),
        Some(first) => SQL_KEYWORDS.contains(&first),
    }
}

/// Whether text opening with `set` sets a database setting rather than a shell
/// option.
///
/// `set` is the one opener the two languages share, and the shell wins on
/// frequency: `set -e && rm -rf ${path}` is how a repository writes a build
/// script, and reading it as SQL filed a shell injection under `CWE-89` with a
/// fix telling the developer to use query placeholders.
///
/// So the second word decides, in the two forms Postgres accepts. Either it is
/// one of [`SQL_SETTINGS`] (`set role ${r}`, `set session ...`), or it is a
/// setting's own name followed by `to` or `=` (`set statement_timeout = '5s'`).
/// A shell option starts with a dash and is neither, and a bare `set` says
/// nothing in either language and is not claimed for SQL.
///
/// `carried` is already lower case; the caller has already established that the
/// first word is `set`.
fn sets_a_database_setting(carried: &str) -> bool {
    let Some(after) = carried.trim_start().strip_prefix("set") else { return false };
    let after = after.trim_start();
    let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.')).collect();
    if name.is_empty() {
        return false;
    }
    if SQL_SETTINGS.contains(&name.as_str()) {
        return true;
    }
    // `name` is ASCII, so its character count is its byte length.
    let rest = after[name.len()..].trim_start();
    if let Some(tail) = rest.strip_prefix("to") {
        return !tail.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
    }
    rest.starts_with('=')
}

/// Whether a value is a regular expression written here: a literal, or
/// `new RegExp(...)`.
fn is_regex_value(node: Node, src: &str) -> bool {
    let node = unwrap(node);
    match node.kind() {
        "regex" => true,
        "new_expression" => node.child_by_field_name("constructor").is_some_and(|c| text(c, src) == "RegExp"),
        _ => false,
    }
}

/// Whether the object a call hangs off is a regular expression, which makes an
/// `exec` on it `RegExp.prototype.exec` and not a SQL driver. A literal and a
/// `new RegExp` are read at the call; a name is resolved one step, in this file,
/// the same way a query built above the call is. See the module doc: on the
/// corpus this was the single largest class of false positives, because
/// `pattern.exec(line)` is how JavaScript matches a string.
fn is_regexp_receiver(call: Node, src: &str, decls: &HashMap<&str, Node>) -> bool {
    let Some(f) = call.child_by_field_name("function").filter(|f| f.kind() == "member_expression") else {
        return false;
    };
    let Some(object) = f.child_by_field_name("object").map(unwrap) else { return false };
    if is_regex_value(object, src) {
        return true;
    }
    object.kind() == "identifier" && decls.get(text(object, src)).is_some_and(|v| is_regex_value(*v, src))
}

/// Whether a declared value is a string, which is what makes a bare identifier
/// at `setTimeout` code rather than the callback it almost always is.
fn is_string_typed(value: Node, src: &str) -> bool {
    let value = unwrap(value);
    match value.kind() {
        "string" | "template_string" => true,
        _ if is_addition(value, src) => {
            let left = value.child_by_field_name("left").map(|n| is_string_typed(n, src)).unwrap_or(false);
            let right = value.child_by_field_name("right").map(|n| is_string_typed(n, src)).unwrap_or(false);
            left || right
        }
        _ => false,
    }
}

/// The callee as written, whitespace collapsed so a call broken across lines
/// still reads as one name in the evidence.
fn callee_text(call: Node, src: &str) -> String {
    let Some(f) = call.child_by_field_name("function") else { return String::new() };
    text(f, src).split_whitespace().collect::<Vec<_>>().join("")
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
fn callee_object<'a>(call: Node<'a>, src: &'a str) -> Option<&'a str> {
    let f = call.child_by_field_name("function")?;
    if f.kind() != "member_expression" {
        return None;
    }
    Some(text(f.child_by_field_name("object")?, src))
}

/// Whether any argument is an options object saying `shell: true` or naming a
/// shell, which is what turns an argv call into a shell call. Node reads the
/// option either way: `shell: "/bin/sh"` runs the command line through a shell
/// exactly as `shell: true` does, and a string is how the option is written
/// whenever the shell has to be chosen.
///
/// The value has to be written here: `true`, a string literal, or a template
/// literal. `shell: process.env.SHELL` reaches a shell just as surely, and this
/// file cannot see that it does, so it is a blind spot rather than a match.
fn has_shell_option(call: Node, src: &str) -> bool {
    args(call).into_iter().map(unwrap).filter(|a| a.kind() == "object").any(|obj| {
        let mut cursor = obj.walk();
        let pairs: Vec<Node> = obj.named_children(&mut cursor).filter(|c| c.kind() == "pair").collect();
        pairs.into_iter().any(|pair| {
            let key = pair.child_by_field_name("key").map(|k| text(k, src).trim_matches(['"', '\'']));
            if key != Some("shell") {
                return false;
            }
            let Some(value) = pair.child_by_field_name("value").map(unwrap) else { return false };
            text(value, src) == "true" || matches!(value.kind(), "string" | "template_string")
        })
    })
}

/// The value of an object argument's `text:` property, which is how the `pg`
/// client takes a query alongside its values.
fn text_property<'a>(node: Node<'a>, src: &'a str) -> Option<Node<'a>> {
    let node = unwrap(node);
    if node.kind() != "object" {
        return None;
    }
    let mut cursor = node.walk();
    let pairs: Vec<Node> = node.named_children(&mut cursor).filter(|c| c.kind() == "pair").collect();
    pairs.into_iter().find_map(|pair| {
        let key = pair.child_by_field_name("key")?;
        if text(key, src).trim_matches(['"', '\'']) != "text" {
            return None;
        }
        pair.child_by_field_name("value")
    })
}

/// What a finding says and which weakness it is filed under.
struct Form {
    evidence: String,
    fix: &'static str,
    cwe: &'static str,
    confidence: Confidence,
}

/// `eval`, `new Function`, and a string passed to `setTimeout`/`setInterval`.
fn code_sink(call: Node, src: &str, decls: &HashMap<&str, Node>) -> Option<Form> {
    let (callee, argument) = if call.kind() == "new_expression" {
        let constructor = call.child_by_field_name("constructor")?;
        if constructor.kind() != "identifier" || text(constructor, src) != "Function" {
            return None;
        }
        // The body is the last argument; everything before it names a parameter.
        ("new Function".to_string(), *args(call).last()?)
    } else {
        let name = callee_name(call, src)?;
        match name {
            "eval" => ("eval".to_string(), *args(call).first()?),
            "setTimeout" | "setInterval" => {
                let first = *args(call).first()?;
                // The argument here is a callback until it is shown to be a
                // string: a function, a method reference and a `.bind` call are
                // the overwhelming majority of what a timer is given. A bare
                // identifier has to be declared a string in this file; anything
                // else has to be one at the call. See the module doc.
                let string_typed = match unwrap(first).kind() {
                    "identifier" => decls.get(text(unwrap(first), src)).is_some_and(|d| is_string_typed(*d, src)),
                    _ => is_string_typed(first, src),
                };
                if !string_typed {
                    return None;
                }
                (name.to_string(), first)
            }
            _ => return None,
        }
    };
    let kind = code_kind(argument, src)?;
    Some(Form {
        evidence: format!("{callee} receives {}", kind.as_str()),
        fix: CODE_FIX,
        cwe: "CWE-95",
        confidence: Confidence::High,
    })
}

/// A command string handed to a shell.
fn command_sink(call: Node, src: &str, decls: &HashMap<&str, Node>) -> Option<Form> {
    if call.kind() != "call_expression" {
        return None;
    }
    let name = callee_name(call, src)?;
    let known_object = match callee_object(call, src) {
        Some(object) => SHELL_OBJECTS.contains(&object),
        None => true,
    };
    let first = *args(call).first()?;
    let reaches_a_shell = if SHELL_CALLEES.contains(&name) {
        // An `exec` on an object this file cannot place is decided by what it
        // carries: a command line is a command line whatever the wrapper is
        // called, and reading `runner.exec(`rm ${x}`)` as SQL because the
        // property is in the SQL set was filing a shell injection under the
        // wrong weakness. A regular expression is neither.
        known_object || (!is_regexp_receiver(call, src, decls) && !carries_sql(first, src))
    } else if ARGV_CALLEES.contains(&name) {
        known_object && has_shell_option(call, src)
    } else {
        false
    };
    if !reaches_a_shell {
        return None;
    }
    let kind = sink_kind(first, src)?;
    // A variable is one step short of evidence: the shape says nothing was
    // interpolated here, only that this file cannot see what was.
    let confidence = if kind == Kind::Variable { Confidence::Medium } else { Confidence::High };
    Some(Form {
        evidence: format!("shell command built from {}", kind.as_str()),
        fix: COMMAND_FIX,
        cwe: "CWE-78",
        confidence,
    })
}

/// A query string handed to a driver.
fn sql_sink(call: Node, src: &str, decls: &HashMap<&str, Node>) -> Option<Form> {
    if call.kind() != "call_expression" || is_tagged_template(call) {
        return None;
    }
    let f = call.child_by_field_name("function")?;
    let named_a_sink = match f.kind() {
        // A bare call is a sink only under the one name that means SQL and
        // nothing else; a bare `query` or `execute` is any function at all.
        "identifier" => text(f, src) == "sql",
        "member_expression" => {
            let property = text(f.child_by_field_name("property")?, src);
            SQL_PROPERTIES.contains(&property)
                // `db.exec` is SQL. `cp.exec` is a shell, which the command
                // family has already claimed, and `pattern.exec` is a regular
                // expression, which is nobody's sink.
                && !(property == "exec"
                    && (callee_object(call, src).is_some_and(|o| SHELL_OBJECTS.contains(&o))
                        || is_regexp_receiver(call, src, decls)))
        }
        _ => false,
    };
    if !named_a_sink {
        return None;
    }
    let first = *args(call).first()?;
    let argument = text_property(first, src).unwrap_or(first);
    let kind = sink_kind(argument, src)?;
    let confidence = match kind {
        Kind::Variable => {
            // One step of resolution, in this file: a query built above and
            // passed down by name is the same finding as one written inline.
            let built = decls.get(text(unwrap(argument), src)).is_some_and(|d| built_from_data(*d, src));
            if built {
                Confidence::High
            } else {
                Confidence::Medium
            }
        }
        _ => Confidence::High,
    };
    Some(Form {
        evidence: format!("SQL built from {} reaches {}", kind.as_str(), callee_text(call, src)),
        fix: SQL_FIX,
        cwe: "CWE-89",
        confidence,
    })
}

fn scan(rule: &InjectionSink, file: &ParsedFile) -> Vec<Finding> {
    let src = &file.source;
    let root = file.tree.root_node();
    let decls = declarations(root, src);
    let mut out: Vec<Finding> = Vec::new();
    walk(root, &mut |n: Node| {
        if !matches!(n.kind(), "call_expression" | "new_expression") {
            return;
        }
        let Some(form) =
            code_sink(n, src, &decls).or_else(|| command_sink(n, src, &decls)).or_else(|| sql_sink(n, src, &decls))
        else {
            return;
        };
        // Interpolated SQL in a test file is advisory. See the module doc.
        let confidence = if form.cwe == "CWE-89" && locrin_core::testcases::is_test_file(&file.rel) {
            Confidence::Medium
        } else {
            form.confidence
        };
        // The evidence joins the symbol in the anchor so that two sinks in one
        // function are two findings rather than one id written twice.
        let at = line(n);
        let anchor = format!("{}\x1f{}", anchor_for(file, at), form.evidence);
        let mut finding = finding_at(rule, &file.rel, line_span(file, at), &anchor, &form.evidence, form.fix);
        finding.confidence = confidence;
        finding.owasp = Some("A03:2021".to_string());
        finding.cwe = Some(form.cwe.to_string());
        out.push(finding);
    });
    out.sort_by_key(|f| (f.span.start_line, f.span.start_col));
    out
}

impl Rule for InjectionSink {
    fn id(&self) -> &'static str {
        "injection-sink"
    }
    fn description(&self) -> &'static str {
        "A built string reaching an evaluator, a shell, or a SQL driver"
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
    /// High is the rule's own answer, which every interpolated and concatenated
    /// sink keeps. A bare identifier is lowered to Medium after construction:
    /// the shape says a value reaches the sink, not that it was built from
    /// data. See the module doc.
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    /// Off by default: **2 of 28 corpus findings were worth acting on**, against
    /// the spec 10.2 gate of 17 in 20. See
    /// `docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md`,
    /// Part B.
    ///
    /// Every one of the 28 is an accurate report of a string built by
    /// interpolation reaching a sink, and that is the problem rather than the
    /// defence: 25 of them are jest seeding a local SQLite fixture with
    /// file-level constants, one applies a committed migration file statement by
    /// statement, and one is a build script whose own comment says the two
    /// interpolated names are hardcoded constants and that there is no injection
    /// surface. All 26 are High severity, so on `strongspan` the rule turned one
    /// High finding into twenty-six and buried the repository's real ones. The
    /// two that stand are `fasting-app`'s v1 importer interpolating table and
    /// column names taken from `Object.keys` of an imported payload.
    ///
    /// Task 10 already lowered a test-file SQL finding to Medium *confidence*
    /// for this cluster, which changes how the finding reads but not how many
    /// there are or what severity blocks the run. Seeing that a fixture's SQL
    /// carries no attacker-reachable value needs the origin of the interpolated
    /// expression, which is cross-function taint and belongs to release two
    /// (spec 4.2). Until then a repository that wants the rule turns it on with
    ///
    /// ```toml
    /// [rules.injection-sink]
    /// enabled = true
    /// ```
    ///
    /// and baselines or `locrin:allow`s its fixtures.
    fn enabled_by_default(&self) -> bool {
        false
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> ParsedFile {
        locrin_core::parse::parse_source(std::path::Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    /// [`carries_sql`] over the value of `const x = ...`.
    fn carried(argument: &str) -> bool {
        let file = parsed(&format!("const x = {argument};\n"));
        let mut answer = None;
        walk(file.tree.root_node(), &mut |n: Node| {
            if n.kind() == "variable_declarator" {
                if let Some(value) = n.child_by_field_name("value") {
                    answer = Some(carries_sql(value, &file.source));
                }
            }
        });
        answer.expect("the fixture declares one value")
    }

    /// A statement is named by the word it opens with. A command line whose
    /// second word is a SQL keyword is still a command line.
    #[test]
    fn only_the_first_word_decides_whether_carried_text_is_sql() {
        assert!(carried("`select * from users where id = ${id}`"));
        assert!(carried("\"INSERT INTO t VALUES (\" + v + \")\""));
        assert!(!carried("`kubectl delete pod ${name}`"), "a command whose second word is a keyword");
        assert!(!carried("`git update-index --refresh ${path}`"));
        assert!(!carried("`rm -rf ${path}`"));
        assert!(carried("q"), "a value this file cannot read falls back to SQL");
    }

    /// A common table expression opens with `with` and the verb comes later, so
    /// the first word has to know about it or the whole statement is filed as a
    /// command line.
    #[test]
    fn a_statement_that_opens_with_a_cte_is_sql() {
        assert!(carried("`with recent as (select id from s where at > ${cutoff}) delete from s where id in (select id from recent)`"));
        assert!(carried("`WITH t AS (SELECT 1) SELECT * FROM t WHERE x = ${x}`"), "any casing");
        assert!(carried("`merge into t using s on t.id = s.id when matched then update set v = ${v}`"));
        assert!(carried("`explain analyze select * from t where id = ${id}`"));
        assert!(carried("`grant select on t to ${role}`"));
        assert!(carried("`revoke select on t from ${role}`"));
        assert!(carried("`set search_path to ${schema}`"));

        // And the sink files it under the SQL weakness rather than the shell
        // one, which is the whole point of the word list.
        let out = scan(
            &InjectionSink,
            &parsed("store.exec(`with recent as (select id from s) delete from s where id in (select id from recent) and o = ${o}`);\n"),
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].cwe.as_deref(), Some("CWE-89"), "{:?}", out[0]);
    }

    /// `set` opens a SQL statement and a shell script alike, so the word on its
    /// own cannot decide. A database `set` names a setting or assigns one; `set
    /// -e` is a shell option, and reading it as SQL filed a shell injection
    /// under CWE-89 with a fix about query placeholders.
    #[test]
    fn set_is_sql_only_when_the_second_word_is_a_setting() {
        assert!(carried("`set search_path to ${schema}`"));
        assert!(carried("`SET ROLE ${role}`"), "any casing");
        assert!(carried("`set local statement_timeout = ${ms}`"));
        assert!(carried("`set session characteristics as transaction read only`"));
        assert!(carried("`set transaction isolation level ${level}`"));
        assert!(carried("`set statement_timeout = '${ms}s'`"), "an assignment names its own setting");
        assert!(carried("`set statement_timeout='${ms}s'`"), "with or without the spaces");
        assert!(carried("`set work_mem to '${mb}MB'`"));

        assert!(!carried("`set -e && rm -rf ${x}`"), "a shell option is not a setting");
        assert!(!carried("`set -- ${args}`"));
        assert!(!carried("`set +x; curl ${url}`"));
        assert!(!carried("`set`"), "a bare set says nothing either way");
    }

    /// The tie-break in place: an `exec` on an object this file cannot place is
    /// filed by the word its command line opens with.
    #[test]
    fn an_unplaceable_exec_is_filed_by_the_first_word_it_carries() {
        let out = scan(&InjectionSink, &parsed("runner.exec(`kubectl delete pod ${name}`);\n"));
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].cwe.as_deref(), Some("CWE-78"), "{:?}", out[0]);

        let out = scan(&InjectionSink, &parsed("store.exec(`insert into t values (${v})`);\n"));
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].cwe.as_deref(), Some("CWE-89"), "{:?}", out[0]);
    }
}
