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
