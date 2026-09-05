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
