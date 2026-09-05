from pathlib import Path

from fp.index import build, candidate_pairs, iter_source_files, read_jsonl, write_jsonl

ROOT = str(Path(__file__).parent / "fixtures" / "repo")


def test_iter_source_files_skips_node_modules():
    files = [Path(f).name for f in iter_source_files([ROOT])]
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
