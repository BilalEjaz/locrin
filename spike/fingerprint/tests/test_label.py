from fp.index import CandidatePair
from fp.label import Label, append_label, pair_key, read_labels, sample_for_labelling


def make_pair(idx: int, structural: bool, jaccard: float) -> CandidatePair:
    return CandidatePair(
        a_id=f"a{idx}", b_id=f"b{idx}",
        a_file=f"a{idx}.ts", b_file=f"b{idx}.ts",
        a_name=f"fnA{idx}", b_name=f"fnB{idx}",
        structural_match=structural, jaccard=jaccard,
        sig_sim=0.5, sig_gate=True,
        a_source="", b_source="",
    )


def build_pairs() -> list[CandidatePair]:
    # 5 structural, 5 high jaccard (0.6 to 1.0), 5 mid jaccard (0.3 to 0.6)
    structural = [make_pair(i, True, 1.0) for i in range(5)]
    high = [make_pair(10 + i, False, 0.6 + i * 0.05) for i in range(5)]
    mid = [make_pair(20 + i, False, 0.3 + i * 0.05) for i in range(5)]
    return structural + high + mid


def bucket_of(p: CandidatePair) -> str:
    if p.structural_match:
        return "structural"
    return "high" if p.jaccard >= 0.6 else "mid"


def test_sample_is_stratified_a_third_from_each_bucket():
    picked = sample_for_labelling(build_pairs(), n=9, seed=7)
    assert len(picked) == 9
    counts = {b: sum(1 for p in picked if bucket_of(p) == b) for b in ("structural", "high", "mid")}
    assert counts == {"structural": 3, "high": 3, "mid": 3}


def test_sample_returns_at_most_n_when_n_is_below_one_per_bucket():
    # n smaller than the three buckets must not be rounded up to one pair per bucket.
    picked = sample_for_labelling(build_pairs(), n=2, seed=7)
    assert len(picked) <= 2


def test_sample_returns_exactly_n_when_the_corpus_is_large_enough():
    # n not divisible by 3 must still deliver n, spreading the remainder across buckets.
    picked = sample_for_labelling(build_pairs(), n=5, seed=7)
    assert len(picked) == 5


def test_sample_is_deterministic_for_a_fixed_seed():
    pairs = build_pairs()
    first = sample_for_labelling(pairs, n=9, seed=11)
    second = sample_for_labelling(pairs, n=9, seed=11)
    assert [pair_key(p) for p in first] == [pair_key(p) for p in second]


def test_sample_never_returns_duplicates():
    # n far larger than the corpus: every bucket is drained, and still no pair repeats.
    picked = sample_for_labelling(build_pairs(), n=100, seed=3)
    keys = [pair_key(p) for p in picked]
    assert len(keys) == len(set(keys)) == 15


def test_sample_survives_empty_buckets():
    only_structural = [make_pair(i, True, 1.0) for i in range(4)]
    picked = sample_for_labelling(only_structural, n=9, seed=5)
    assert len(picked) == 3 and all(p.structural_match for p in picked)


def test_pair_key_is_order_independent():
    forward = make_pair(1, True, 1.0)
    reversed_pair = make_pair(1, True, 1.0)
    reversed_pair.a_id, reversed_pair.b_id = forward.b_id, forward.a_id
    assert pair_key(forward) == pair_key(reversed_pair)


def test_append_label_then_read_labels_round_trips(tmp_path):
    path = str(tmp_path / "nested" / "labels.jsonl")
    assert read_labels(path) == {}
    one = Label(pair_key="a1|b1", label="dup", reason="same body")
    two = Label(pair_key="a2|b2", label="not", reason="different intent")
    append_label(path, one)
    append_label(path, two)
    labels = read_labels(path)
    assert labels == {"a1|b1": one, "a2|b2": two}


def test_read_labels_keeps_the_last_verdict_for_a_pair(tmp_path):
    # The loop only ever appends, so a re-labelled pair must resolve to the later line.
    path = str(tmp_path / "labels.jsonl")
    append_label(path, Label(pair_key="a1|b1", label="unsure", reason="first pass"))
    append_label(path, Label(pair_key="a1|b1", label="dup", reason="second pass"))
    assert read_labels(path) == {"a1|b1": Label("a1|b1", "dup", "second pass")}
