from fp.signature import gate, signature_of, similarity

A = """async function load(id: string) {
  const res = await fetch(`/api/${id}`);
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

A2 = """async function fetchOne(key: string) {
  const r = await fetch(`/v2/${key}`);
  const body = await r.json();
  cache.set(key, body);
  return body;
}"""

B = """function fmt(d: Date, locale: string) {
  return d.toLocaleDateString(locale);
}"""

# extract.FunctionRecord.source is the exact node text, so a method arrives as a bare
# method body and an arrow arrives without any surrounding declaration.
METHOD = """async load(id: string) {
  const res = await fetch(`/api/${id}`);
  cache.set(id, res);
  return res;
}"""

ARROW = """async (id: string) => {
  const res = await fetch(`/api/${id}`);
  cache.set(id, res);
  return res;
}"""


def test_signature_fields():
    s = signature_of(A)
    assert s.param_count == 1
    assert s.is_async is True
    assert s.returns_value is True
    assert {"fetch", "json", "set"} <= set(s.callees)


def test_similar_signatures_score_high_and_gate_passes():
    assert similarity(signature_of(A), signature_of(A2)) > 0.8
    assert gate(signature_of(A), signature_of(A2)) is True


def test_bare_method_source_is_understood():
    s = signature_of(METHOD)
    assert s.param_count == 1
    assert s.is_async is True
    assert s.returns_value is True
    assert {"fetch", "set"} <= set(s.callees)
    assert "load" not in s.callees


def test_arrow_source_is_understood():
    s = signature_of(ARROW)
    assert s.param_count == 1
    assert s.is_async is True
    assert s.returns_value is True
    assert {"fetch", "set"} <= set(s.callees)


def test_different_signatures_score_low_and_gate_fails():
    assert similarity(signature_of(A), signature_of(B)) < 0.4
    assert gate(signature_of(A), signature_of(B)) is False
