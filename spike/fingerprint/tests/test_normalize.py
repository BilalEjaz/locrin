from fp.normalize import MIN_TOKENS, structural_hash, tokens

A = """function add(a: number, b: number): number {
  const total = a + b;
  console.log("adding", total);
  return total;
}"""

A_RENAMED = """function sum(x: number, y: number): number {
  const result = x + y;
  console.log("summing", result);
  return result;
}"""

B = """function add(a: number, b: number): number {
  const total = a + b;
  if (total > 10) { return 10; }
  return total;
}"""


def test_identifiers_and_literals_are_placeholders():
    t = tokens(A)
    assert "ID" in t and "LIT" in t
    assert "add" not in t and "total" not in t and '"adding"' not in t


def test_renamed_copy_has_same_structural_hash():
    assert structural_hash(tokens(A)) == structural_hash(tokens(A_RENAMED))


def test_changed_body_has_different_hash():
    assert structural_hash(tokens(A)) != structural_hash(tokens(B))


def test_min_tokens_constant():
    assert MIN_TOKENS == 40


# extract.FunctionRecord.source is the exact node text, so a method arrives as a bare
# method body, which is not valid standalone TypeScript.
METHOD = """async load(id: string) {
  const r = await fetch(id);
  const d = await r.json();
  cache.set(id, d);
  return d;
}"""

METHOD_RENAMED = """async fetchOne(key: string) {
  const res = await fetch(key);
  const body = await res.json();
  store.set(key, body);
  return body;
}"""


def test_bare_method_tokenises_without_error_nodes():
    assert "ERROR" not in tokens(METHOD)


def test_renamed_bare_method_has_same_structural_hash():
    assert structural_hash(tokens(METHOD)) == structural_hash(tokens(METHOD_RENAMED))


def test_bare_method_tokens_exclude_class_wrapper():
    t = tokens(METHOD)
    assert t[0] == "method_definition"
    assert "class_declaration" not in t
    assert "class_body" not in t
    assert "class" not in t
