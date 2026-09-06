from fp.minhash import LshIndex, estimated_jaccard, minhash_of, shingles
from fp.normalize import tokens

BASE = """function load(id: string) {
  const res = await fetch(`/api/items/${id}`);
  if (!res.ok) { throw new Error("failed"); }
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

NEAR = """function loadItem(itemId: string) {
  const res = await fetch(`/api/items/${itemId}`);
  if (!res.ok) { throw new Error("load failed"); }
  const data = await res.json();
  cache.set(itemId, data);
  console.log("loaded", itemId);
  return data;
}"""

FAR = """function formatDate(d: Date) {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}"""


def test_shingles_are_k_grams():
    s = shingles(["a", "b", "c", "d", "e", "f"], k=5)
    assert s == {"a b c d e", "b c d e f"}


def test_near_copy_is_similar_and_far_is_not():
    mb, mn, mf = (minhash_of(tokens(x)) for x in (BASE, NEAR, FAR))
    assert estimated_jaccard(mb, mn) > 0.6
    assert estimated_jaccard(mb, mf) < 0.3


def test_lsh_retrieves_near_not_far():
    idx = LshIndex(threshold=0.5)
    idx.add("near", minhash_of(tokens(NEAR)))
    idx.add("far", minhash_of(tokens(FAR)))
    hits = idx.query(minhash_of(tokens(BASE)))
    assert "near" in hits and "far" not in hits
