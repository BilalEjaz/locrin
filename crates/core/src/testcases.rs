//! Test cases found in a parsed test file: their names, whether the runner will
//! skip them, and how many assertions each one makes.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::parse::ParsedFile;
use crate::tree::{line, text};

/// The name given to a case whose first argument is not a string literal, such as
/// `it(nameFor(x), ...)`. Rules report it as written rather than guessing.
const UNNAMED: &str = "<unnamed>";

/// Bare callees that start a test case.
const CASE_IDENTS: [&str; 6] = ["it", "test", "xit", "xtest", "fit", "ftest"];
/// Properties of `it`/`test` that still start a test case.
const CASE_PROPS: [&str; 5] = ["skip", "only", "todo", "failing", "concurrent"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// The first argument's literal text, or `<unnamed>` when it is not a literal.
    pub name: String,
    pub line: u32,
    pub end_line: u32,
    pub skipped: bool,
    pub assertions: u32,
}

/// Whether a repo-relative path is a test file by naming convention: `*.test.*`,
/// `*.spec.*`, or anything under a `__tests__` directory.
pub fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.contains(".test.") || name.contains(".spec.") || rel.split('/').any(|seg| seg == "__tests__")
}

/// Every test case in `file`, in source order.
pub fn extract(file: &ParsedFile) -> Vec<TestCase> {
    let src = &file.source;
    let root = file.tree.root_node();
    let helpers = assertion_helpers(root, src);
    let mut out = Vec::new();
    visit(root, &mut |node| {
        if node.kind() != "call_expression" {
            return;
        }
        let Some(callee) = case_callee(node, src) else { return };
        let args = named_args(node);
        let body = args.get(1).filter(|a| matches!(a.kind(), "arrow_function" | "function_expression"));
        out.push(TestCase {
            name: case_name(args.first().copied(), src),
            line: line(node),
            end_line: node.end_position().row as u32 + 1,
            skipped: callee_skips(callee, src) || in_skipped_suite(node, src),
            assertions: body.map_or(0, |b| count_assertions(*b, src, &helpers)),
        });
    });
    out
}

/// Pre-order walk, `node` first. Source order, so callers can push as they go.
fn visit<'a>(node: Node<'a>, f: &mut impl FnMut(Node<'a>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, f);
    }
}

/// The callee node when `call` starts a test case, or None when it does not. The
/// node returned is the one that decides `skipped`: for `it.each(table)(...)` that
/// is the inner `it.each`, not the outer call.
fn case_callee<'a>(call: Node<'a>, src: &str) -> Option<Node<'a>> {
    let func = call.child_by_field_name("function")?;
    match func.kind() {
        "identifier" => CASE_IDENTS.contains(&text(func, src)).then_some(func),
        "member_expression" => {
            let (obj, prop) = object_and_property(func, src)?;
            (matches!(obj, "it" | "test") && CASE_PROPS.contains(&prop)).then_some(func)
        }
        // `it.each(table)("name", fn)`: the outer call carries the name and callback.
        "call_expression" => {
            let inner = func.child_by_field_name("function")?;
            if inner.kind() != "member_expression" {
                return None;
            }
            let (obj, prop) = object_and_property(inner, src)?;
            (matches!(obj, "it" | "test") && prop == "each").then_some(inner)
        }
        _ => None,
    }
}

/// The texts of a `member_expression`'s object and property, when the object is a
/// plain identifier. `a.b.c` yields None, since its object is itself a member.
fn object_and_property<'a>(member: Node, src: &'a str) -> Option<(&'a str, &'a str)> {
    let obj = member.child_by_field_name("object")?;
    let prop = member.child_by_field_name("property")?;
    (obj.kind() == "identifier").then(|| (text(obj, src), text(prop, src)))
}

/// Whether the callee itself marks the case as skipped.
fn callee_skips(callee: Node, src: &str) -> bool {
    match callee.kind() {
        "identifier" => matches!(text(callee, src), "xit" | "xtest"),
        "member_expression" => {
            callee.child_by_field_name("property").is_some_and(|p| matches!(text(p, src), "skip" | "todo"))
        }
        _ => false,
    }
}

/// Whether any enclosing suite call is `describe.skip`, `context.skip`, or `xdescribe`.
fn in_skipped_suite(node: Node, src: &str) -> bool {
    let mut current = node.parent();
    while let Some(n) = current {
        if n.kind() == "call_expression" {
            if let Some(func) = n.child_by_field_name("function") {
                let skipped = match func.kind() {
                    "identifier" => text(func, src) == "xdescribe",
                    "member_expression" => object_and_property(func, src)
                        .is_some_and(|(obj, prop)| matches!(obj, "describe" | "context") && prop == "skip"),
                    _ => false,
                };
                if skipped {
                    return true;
                }
            }
        }
        current = n.parent();
    }
    false
}

/// A call's arguments, comments dropped so positions stay meaningful.
fn named_args<'a>(call: Node<'a>) -> Vec<Node<'a>> {
    let Some(args) = call.child_by_field_name("arguments") else { return Vec::new() };
    let mut cursor = args.walk();
    args.named_children(&mut cursor).filter(|c| c.kind() != "comment").collect()
}

fn case_name(arg: Option<Node>, src: &str) -> String {
    let Some(arg) = arg else { return UNNAMED.to_string() };
    match arg.kind() {
        "string" => {
            let mut cursor = arg.walk();
            let fragment = arg.named_children(&mut cursor).find(|c| c.kind() == "string_fragment");
            fragment.map_or(String::new(), |f| text(f, src).to_string())
        }
        // Interpolations are kept as written: `it(`case ${n}`)` names itself that way.
        "template_string" => {
            let raw = text(arg, src);
            raw.strip_prefix('`').and_then(|s| s.strip_suffix('`')).unwrap_or(raw).to_string()
        }
        _ => UNNAMED.to_string(),
    }
}

/// Same-file functions whose own body asserts. A call to one of these counts as an
/// assertion, so a case that delegates its checks to a shared helper is not read as
/// assertion-free. One level only: helpers calling helpers are not chased.
fn assertion_helpers(root: Node, src: &str) -> HashSet<String> {
    let none = HashSet::new();
    let mut helpers = HashSet::new();
    visit(root, &mut |node| {
        let (name, body) = match node.kind() {
            "function_declaration" => (node.child_by_field_name("name"), node.child_by_field_name("body")),
            "variable_declarator" => match node.child_by_field_name("value") {
                Some(value) if matches!(value.kind(), "arrow_function" | "function_expression") => {
                    (node.child_by_field_name("name"), value.child_by_field_name("body"))
                }
                _ => (None, None),
            },
            _ => (None, None),
        };
        let (Some(name), Some(body)) = (name, body) else { return };
        if name.kind() == "identifier" && count_assertions(body, src, &none) > 0 {
            helpers.insert(text(name, src).to_string());
        }
    });
    helpers
}

fn count_assertions(node: Node, src: &str, helpers: &HashSet<String>) -> u32 {
    let mut count = 0;
    visit(node, &mut |n| {
        if asserts(n, src, helpers) {
            count += 1;
        }
    });
    count
}

/// Whether a single node is an assertion. Counting per node rather than per
/// statement keeps `expect(x).toBe(1)` at one: the outer call's callee is a member
/// whose leftmost part is a call, not the `expect` identifier.
fn asserts(node: Node, src: &str, helpers: &HashSet<String>) -> bool {
    match node.kind() {
        "call_expression" => {
            let Some(func) = node.child_by_field_name("function") else { return false };
            match func.kind() {
                "identifier" => {
                    let name = text(func, src);
                    matches!(name, "expect" | "assert") || helpers.contains(name)
                }
                // `expect.assertions(1)`, `assert.equal(a, b)`.
                "member_expression" => {
                    leftmost_identifier(func).is_some_and(|id| matches!(text(id, src), "expect" | "assert"))
                }
                _ => false,
            }
        }
        // Chai's `value.should.equal(1)`.
        "member_expression" => node.child_by_field_name("property").is_some_and(|p| text(p, src) == "should"),
        _ => false,
    }
}

/// The identifier a member chain starts from: `a.b.c` gives `a`, `f().b` gives None.
fn leftmost_identifier<'a>(member: Node<'a>) -> Option<Node<'a>> {
    let mut current = member;
    loop {
        match current.kind() {
            "identifier" => return Some(current),
            "member_expression" => current = current.child_by_field_name("object")?,
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_source;
    use std::path::Path;

    const SRC: &str = r#"import { render } from "@testing-library/react";

function checkShape(value: unknown) {
  expect(value).toBeDefined();
}

const makeName = () => "generated";

describe("suite", () => {
  it("renders a row", () => {
    const { getByText } = render(<div>hi</div>);
    expect(getByText("hi")).toBeTruthy();
  });

  it.only("only case", () => {
    expect(5).toBe(5);
  });

  test.skip("skipped by property", () => {
    expect(1).toBe(1);
  });

  xit("skipped by prefix", () => {
    expect(2).toBe(2);
  });

  it.todo("todo case");

  it.each([[1], [2]])("each case %i", (n) => {
    expect(n).toBeGreaterThan(0);
  });

  it(`template name`, () => {
    checkShape({ a: 1 });
  });

  it("chai style", () => {
    assert.equal(1, 1);
    const result = 1;
    result.should.equal(1);
  });

  it("no assertions", () => {
    render(<div />);
  });

  it(makeName(), () => {
    expect.assertions(1);
    expect(3).toBe(3);
  });
});

describe.skip("skipped suite", () => {
  it("inside a skipped describe", () => {
    expect(4).toBe(4);
  });
});
"#;

    fn parsed(src: &str) -> ParsedFile {
        parse_source(Path::new("src/a.test.tsx"), "src/a.test.tsx", src.to_string()).unwrap()
    }

    #[test]
    fn extracts_every_case_form_with_names_lines_skips_and_assertions() {
        let file = parsed(SRC);
        assert!(!file.has_error, "fixture must parse cleanly");
        let cases = extract(&file);
        let view: Vec<(&str, u32, bool, u32)> =
            cases.iter().map(|c| (c.name.as_str(), c.line, c.skipped, c.assertions)).collect();
        assert_eq!(
            view,
            vec![
                ("renders a row", 10, false, 1),
                ("only case", 15, false, 1),
                ("skipped by property", 19, true, 1),
                ("skipped by prefix", 23, true, 1),
                ("todo case", 27, true, 0),
                ("each case %i", 29, false, 1),
                ("template name", 33, false, 1),
                ("chai style", 37, false, 2),
                ("no assertions", 43, false, 0),
                ("<unnamed>", 47, false, 2),
                ("inside a skipped describe", 54, true, 1),
            ]
        );
        assert_eq!(cases[0].end_line, 13);
    }

    #[test]
    fn suite_level_skips_cover_xdescribe_and_context_skip() {
        let src = r#"xdescribe("a", () => {
  it("one", () => { expect(1).toBe(1); });
});
context.skip("b", () => {
  it("two", () => { expect(2).toBe(2); });
});
describe("c", () => {
  it("three", () => { expect(3).toBe(3); });
});
"#;
        let cases = extract(&parsed(src));
        let view: Vec<(&str, bool)> = cases.iter().map(|c| (c.name.as_str(), c.skipped)).collect();
        assert_eq!(view, vec![("one", true), ("two", true), ("three", false)]);
    }

    #[test]
    fn is_test_file_matches_the_three_conventions() {
        for rel in
            ["src/a.test.ts", "src/a.test.tsx", "src/a.spec.js", "src/__tests__/a.ts", "__tests__/a.ts", "a.spec.tsx"]
        {
            assert!(is_test_file(rel), "{rel} should be a test file");
        }
        for rel in ["src/a.ts", "src/latest.ts", "src/tests/a.ts", "e2e/login.ts", "src/testing.ts", "src/spec.ts"] {
            assert!(!is_test_file(rel), "{rel} should not be a test file");
        }
    }
}
