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

/// The names of the cases `file` skips, or nothing at all when `file` is not a
/// test file.
///
/// This is what the index stores per file so a later run can tell a newly
/// skipped case from one that was already skipped. It lives here, beside the
/// extractor, because it is a tree walk: callers run it in whatever parallel
/// pass already holds the parsed file rather than on the thread that owns the
/// index connection. See [`crate::indexer::record_with_stat`].
pub fn skipped_names(file: &ParsedFile) -> Vec<String> {
    if !is_test_file(&file.rel) {
        return Vec::new();
    }
    extract(file).into_iter().filter(|c| c.skipped).map(|c| c.name).collect()
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
        let body = case_body(&args);
        out.push(TestCase {
            name: case_name(args.first().copied(), src),
            line: line(node),
            end_line: node.end_position().row as u32 + 1,
            skipped: callee_skips(callee, src) || in_skipped_suite(node, src),
            assertions: body.map_or(0, |b| count_assertions(b, src, &helpers)),
        });
    });
    out
}

/// The callback a case runs, which is the last function among the arguments
/// after the name. Runners let a case carry an options object or a timeout
/// around its callback (`test("n", { timeout: 5000 }, fn)`, `it("n", fn, 10000)`),
/// so a fixed argument position would miss the body of the first form and read
/// zero assertions for it. Argument zero is the name and is never the body: a
/// case written `it(() => {})` has no name and no body worth counting.
fn case_body<'a>(args: &[Node<'a>]) -> Option<Node<'a>> {
    args.iter().skip(1).rev().find(|a| matches!(a.kind(), "arrow_function" | "function_expression")).copied()
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

/// Query prefixes that throw when nothing matches. Testing Library's `getBy*`
/// and `getAllBy*` throw straight away and `findBy*` / `findAllBy*` return a
/// promise that rejects, so a call to one of them fails the case when the thing
/// it looks for is not there: it is an assertion however it is spelled.
/// `queryBy*` and `queryAllBy*` are deliberately absent, because they return
/// null rather than throwing and check nothing on their own.
const THROWING_QUERY_PREFIXES: [&str; 4] = ["getBy", "getAllBy", "findBy", "findAllBy"];

fn is_throwing_query(name: &str) -> bool {
    THROWING_QUERY_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Whether a single node is an assertion. Counting per node rather than per
/// statement keeps `expect(x).toBe(1)` at one: the outer call's callee is a member
/// whose leftmost part is a call, not the `expect` identifier.
fn asserts(node: Node, src: &str, helpers: &HashSet<String>) -> bool {
    match node.kind() {
        // A case that throws fails, so a `throw` is a check: the guard style
        // `if (!ok) throw new Error("<why>")` asserts exactly what an `expect`
        // would and names the offending item in the message besides. It counts
        // wherever it sits in the body, nested blocks and loops included, which
        // is where the style is written. All 11 of `test-no-assert`'s corpus
        // findings were that shape
        // (`docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md`).
        // The cost: a rethrow inside a catch reads the same, and telling the two
        // apart needs to know whether the try body can fail.
        "throw_statement" => true,
        "call_expression" => {
            let Some(func) = node.child_by_field_name("function") else { return false };
            match func.kind() {
                "identifier" => {
                    let name = text(func, src);
                    matches!(name, "expect" | "assert") || is_throwing_query(name) || helpers.contains(name)
                }
                // `expect.assertions(1)`, `assert.equal(a, b)`, and the throwing
                // queries as they are usually written: `screen.getByText(...)`,
                // `within(row).getAllByRole(...)`. The property is read on its
                // own because the object of the second form is a call, so there
                // is no leftmost identifier to recognise.
                "member_expression" => {
                    leftmost_identifier(func).is_some_and(|id| matches!(text(id, src), "expect" | "assert"))
                        || func.child_by_field_name("property").is_some_and(|p| is_throwing_query(text(p, src)))
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
                // Two: the `expect(...)` and the `getByText("hi")` inside it,
                // which throws on its own. Rules ask whether a case asserts at
                // all, so counting a nested throwing query twice over is
                // harmless; missing it entirely was not.
                ("renders a row", 10, false, 2),
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

    /// Vitest and recent Jest let a case carry an options object between its name
    /// and its callback. Reading argument one as the body counted zero assertions
    /// for every such case, which is a false positive for any rule that asks
    /// whether a case asserts, so the body is the last function argument instead.
    #[test]
    fn the_body_is_the_last_function_argument_not_the_second() {
        let src = r#"test("options object before the body", { timeout: 5000 }, () => {
  expect(1).toBe(1);
});

it("trailing timeout after the body", () => {
  expect(2).toBe(2);
}, 10000);

it("no callback at all", { timeout: 5000 });

it(() => {
  expect(3).toBe(3);
});
"#;
        let cases = extract(&parsed(src));
        let view: Vec<(&str, u32)> = cases.iter().map(|c| (c.name.as_str(), c.assertions)).collect();
        assert_eq!(
            view,
            vec![
                ("options object before the body", 1),
                ("trailing timeout after the body", 1),
                ("no callback at all", 0),
                // The name slot holds the callback, so the case has no body: a
                // rule reports `<unnamed>` with nothing asserted rather than
                // reading the name argument as a body.
                ("<unnamed>", 0),
            ]
        );
    }

    /// Testing Library's `getBy*` / `findBy*` queries throw when nothing matches,
    /// so a case whose only check is one of them is fully checked and asserts.
    /// `queryBy*` returns null instead and checks nothing on its own.
    #[test]
    fn throwing_queries_assert_and_query_by_does_not() {
        let src = r#"it("bare getBy", () => {
  getByText("hi");
});

it("screen member", () => {
  screen.getByRole("button");
});

it("within a scope", () => {
  within(row).getAllByTestId("cell");
});

it("awaited find", async () => {
  await findByText("hi");
});

it("awaited find all through screen", async () => {
  await screen.findAllByText("hi");
});

it("query only", () => {
  const found = queryByText("hi");
  void found;
});

it("query all through screen only", () => {
  screen.queryAllByText("hi");
});

it("waits on a throwing query", async () => {
  await waitFor(() => screen.getByText("hi"));
});
"#;
        let cases = extract(&parsed(src));
        let view: Vec<(&str, u32)> = cases.iter().map(|c| (c.name.as_str(), c.assertions)).collect();
        assert_eq!(
            view,
            vec![
                ("bare getBy", 1),
                ("screen member", 1),
                ("within a scope", 1),
                ("awaited find", 1),
                ("awaited find all through screen", 1),
                ("query only", 0),
                ("query all through screen only", 0),
                ("waits on a throwing query", 1),
            ]
        );
    }

    /// A guard case walks a data set and throws a written explanation when an
    /// invariant breaks. The runner reports a thrown error as a failed case, so
    /// the case checks exactly as much as an `expect` would; it just names no
    /// matcher. The `throw` counts wherever it sits in the body.
    #[test]
    fn a_throw_anywhere_in_the_body_is_an_assertion() {
        let src = r#"const ITEMS = [{ id: "a" }, { id: "b" }];

function assertShape(item: { id: string }) {
  if (!item.id) {
    throw new Error("every item needs an id");
  }
}

it("throws at the top level of the body", () => {
  throw new Error("unreachable");
});

it("throws inside an if", () => {
  const found = ITEMS[0];
  if (!found) {
    throw new Error("missing item");
  }
});

it("throws inside a loop nested in a block", () => {
  for (const item of ITEMS) {
    if (item.id.length === 0) {
      throw new Error(`empty id for ${JSON.stringify(item)}`);
    }
  }
});

it("delegates to a helper that throws", () => {
  assertShape(ITEMS[0]);
});

it("catches its own throw and checks nothing", () => {
  try {
    JSON.parse("{}");
  } catch (err) {
    throw new Error(String(err));
  }
});

it("checks nothing at all", () => {
  ITEMS.map((item) => item.id);
});
"#;
        let cases = extract(&parsed(src));
        let view: Vec<(&str, u32)> = cases.iter().map(|c| (c.name.as_str(), c.assertions)).collect();
        assert_eq!(
            view,
            vec![
                ("throws at the top level of the body", 1),
                ("throws inside an if", 1),
                ("throws inside a loop nested in a block", 1),
                // A same-file helper whose own body throws is an assertion
                // helper, on the same one level the `expect` helpers get.
                ("delegates to a helper that throws", 1),
                // Read as an assertion, which is the honest cost of a syntax
                // rule: a rethrow inside a catch fails the case too, and
                // telling it from a guard needs to know the try body can
                // throw. Pinned so the cost is visible rather than assumed.
                ("catches its own throw and checks nothing", 1),
                ("checks nothing at all", 0),
            ]
        );
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
