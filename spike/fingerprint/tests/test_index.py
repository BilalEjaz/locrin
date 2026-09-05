import random
from pathlib import Path

from fp.extract import FunctionRecord
from fp.index import Indexed, build, candidate_pairs, iter_source_files, read_jsonl, write_jsonl
from fp.minhash import minhash_of
from fp.normalize import structural_hash
from fp.signature import Signature

ROOT = str(Path(__file__).parent / "fixtures" / "repo")


def _mutated_corpus(n: int = 120, seed: int = 11) -> list[Indexed]:
    """A corpus of loosely related functions, which is what a real repo looks like.

    The three-function fixture repo is too small to show LSH's behaviour below its
    nominal threshold: with so few candidates nothing collides by chance. Mutating one
    base token stream reproduces it deterministically (seeded rng, and datasketch's
    MinHash permutations are seeded too).
    """
    rng = random.Random(seed)
    vocab = ["ID", "LIT", "if_statement", "return_statement", "call_expression",
             "member_expression", "await_expression", "{", "}"]
    base = [rng.choice(vocab) for _ in range(90)]
    items: list[Indexed] = []
    for i in range(n):
        toks = list(base)
        for _ in range(28):
            toks[rng.randrange(len(toks))] = rng.choice(vocab)
        rec = FunctionRecord(id=f"f{i}", file=f"/x/f{i}.ts", name=f"fn{i}",
                             start_line=1, end_line=9, source="x")
        items.append(Indexed(rec, toks, structural_hash(toks), minhash_of(toks),
                             Signature(1, frozenset(), True, False)))
    return items


def test_iter_source_files_skips_node_modules():
    files = [Path(f).name for f in iter_source_files([ROOT])]
    assert set(files) == {"a.ts", "b.ts"}


def test_iter_source_files_skips_test_code():
    files = [Path(f).name for f in iter_source_files([ROOT])]
    assert "helper.test.ts" not in files  # under a __tests__ directory
    assert "c.spec.ts" not in files  # .spec. in the file name
    assert set(files) == {"a.ts", "b.ts"}


def test_build_filters_small_functions():
    items = build([ROOT])
    names = {i.record.name for i in items}
    assert "loadUser" in names and "loadProfile" in names and "unrelated" in names
    assert "tinyHelper" not in names  # under MIN_TOKENS, dropped by build()


def test_candidate_pairs_find_the_near_duplicate(tmp_path):
    items = build([ROOT])
    pairs = candidate_pairs(items, floor=0.3)
    names = {frozenset((p.a_name, p.b_name)) for p in pairs}
    assert frozenset(("loadUser", "loadProfile")) in names
    hit = next(p for p in pairs if {p.a_name, p.b_name} == {"loadUser", "loadProfile"})
    assert hit.jaccard > 0.5 and hit.sig_gate is True
    out = tmp_path / "c.jsonl"
    write_jsonl(pairs, str(out))
    assert len(read_jsonl(str(out))) == len(pairs)


def test_candidate_pairs_drop_lsh_pairs_under_the_floor():
    items = build([ROOT])
    pairs = candidate_pairs(items, floor=0.3)
    names = {frozenset((p.a_name, p.b_name)) for p in pairs}
    assert frozenset(("unrelated", "loadUser")) not in names
    assert frozenset(("unrelated", "loadProfile")) not in names
    assert all(p.structural_match or p.jaccard >= 0.3 for p in pairs)


def test_candidate_pairs_drop_sub_floor_pairs_in_a_larger_corpus():
    items = _mutated_corpus()
    pairs = candidate_pairs(items, floor=0.3)
    assert all(p.structural_match or p.jaccard >= 0.3 for p in pairs)
