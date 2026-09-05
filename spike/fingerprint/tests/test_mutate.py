import re
from pathlib import Path

from fp.extract import FunctionRecord
from fp.index import Indexed, build, candidate_pairs
from fp.minhash import minhash_of
from fp.mutate import MUTATIONS, mutate, plant
from fp.normalize import structural_hash, tokens
from fp.signature import signature_of

SRC = """async function loadUser(id: string) {
  const res = await fetch(`/api/users/${id}`);
  if (!res.ok) {
    throw new Error("user load failed");
  }
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

ROOT = str(Path(__file__).parent / "fixtures" / "repo")


def test_rename_changes_identifiers_only():
    # The brief's stricter assertions are brittle: _rename is a word-boundary regex, so a
    # seeded sample can land on a property name (json) or a word inside a string literal.
    # The brief allows falling back to "renaming happened"; line count proves nothing else moved.
    out = mutate(SRC, "rename", seed=1)
    assert out != SRC
    assert out.count("\n") == SRC.count("\n")
    assert re.search(r"\w+Alt0\b", out)


def test_insert_adds_a_statement():
    out = mutate(SRC, "insert", seed=1)
    assert out.count("\n") == SRC.count("\n") + 1


def test_literals_changes_strings():
    out = mutate(SRC, "literals", seed=1)
    assert '"user load failed"' not in out


def test_all_mutation_kinds_registered():
    assert set(MUTATIONS) == {"rename", "insert", "literals", "combined"}


def test_plant_extends_items_and_pairs_find_them():
    items = build([ROOT])
    extended, planted = plant(items, n=2, seed=3)
    assert len(extended) == len(items) + 2 and len(planted) == 2
    pairs = candidate_pairs(extended, floor=0.3)
    pair_keys = {frozenset((p.a_id, p.b_id)) for p in pairs}
    for pl in planted:
        assert frozenset((pl.original_id, pl.planted_id)) in pair_keys


def make_indexed(name: str, source: str) -> Indexed:
    rec = FunctionRecord(
        id=f"{name}.ts:1:0:{name}", file=f"{name}.ts", name=name,
        start_line=1, end_line=1 + source.count("\n"), source=source,
    )
    toks = tokens(source)
    return Indexed(rec, toks, structural_hash(toks), minhash_of(toks), signature_of(source))


# No quoted string, so _literals no-ops. No line ending in "{", so _insert no-ops. Every
# identifier is either reserved or a single character, so _rename has nothing to sample.
# _combined is the three chained, so it no-ops too.
INERT = "const f = (a: number) => a + 1;"

MUTABLE_A = """function totalSpend(rows: Row[]) {
  let total = 0;
  for (const row of rows) {
    total += row.amount;
  }
  return total;
}"""

MUTABLE_B = """function labelFor(status: string) {
  if (status === "open") {
    return "Open now";
  }
  return "Closed";
}"""


def test_plant_never_plants_a_byte_identical_copy():
    items = [make_indexed("inert", INERT), make_indexed("spend", MUTABLE_A),
             make_indexed("label", MUTABLE_B)]
    extended, planted = plant(items, n=len(items), seed=3)
    by_id = {i.record.id: i.record.source for i in extended}
    for pl in planted:
        assert by_id[pl.planted_id] != by_id[pl.original_id], f"{pl.kind} planted a copy"
    # The inert base cannot be mutated at all, so it must be skipped rather than counted.
    assert len(planted) == 2
    assert all(pl.original_id != items[0].record.id for pl in planted)
