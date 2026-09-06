from fp.normalize import tokens
from fp.parse import parse_snippet

FUNC = """function add(a: number, b: number): number {
  return a + b;
}"""

METHOD = """async load(id: string) {
  const r = await fetch(id);
  const d = await r.json();
  cache.set(id, d);
  return d;
}"""


def test_plain_function_snippet_parses_cleanly():
    assert parse_snippet(FUNC).root_node.has_error is False


def test_bare_method_snippet_parses_cleanly():
    assert parse_snippet(METHOD).root_node.has_error is False


# A .tsx component: JSX needs the TSX grammar, and it must not get the class wrapper.
JSX = """function Row({ item }: { item: string }) {
  const label = item.trim();
  return <div className="row">{label}</div>;
}"""


def test_jsx_component_snippet_parses_cleanly():
    assert parse_snippet(JSX).root_node.has_error is False


def test_jsx_component_tokenises_without_error_or_wrapper():
    t = tokens(JSX)
    assert "ERROR" not in t
    assert "class_declaration" not in t
