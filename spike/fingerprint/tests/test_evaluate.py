from fp.evaluate import choose, precision_at, predicted, recall_at, recall_by_kind, sweep
from fp.index import CandidatePair
from fp.label import Label, pair_key
from fp.mutate import Planted


def cp(a, b, structural=False, jaccard=0.0, gate=True):
    return CandidatePair(a_id=a, b_id=b, a_file="fa", b_file="fb", a_name=a, b_name=b,
                         structural_match=structural, jaccard=jaccard, sig_sim=0.5, sig_gate=gate,
                         a_source="", b_source="")


def test_predicted_rules():
    assert predicted(cp("a", "b", structural=True), t=0.9, require_gate=True)
    assert predicted(cp("a", "b", jaccard=0.7), t=0.6, require_gate=True)
    assert not predicted(cp("a", "b", jaccard=0.5), t=0.6, require_gate=True)
    assert not predicted(cp("a", "b", jaccard=0.7, gate=False), t=0.6, require_gate=True)
    assert predicted(cp("a", "b", jaccard=0.7, gate=False), t=0.6, require_gate=False)


def test_recall_and_precision():
    pairs = [cp("o1", "p1", jaccard=0.8), cp("o2", "p2", jaccard=0.4), cp("x", "y", jaccard=0.9), cp("q", "r", jaccard=0.7)]
    planted = [Planted("o1", "p1", "rename"), Planted("o2", "p2", "combined")]
    assert recall_at(pairs, planted, t=0.6, require_gate=True) == 0.5
    assert recall_at(pairs, planted, t=0.3, require_gate=True) == 1.0
    labels = {
        pair_key(pairs[2]): Label(pair_key(pairs[2]), "dup", ""),
        pair_key(pairs[3]): Label(pair_key(pairs[3]), "not", ""),
        pair_key(pairs[1]): Label(pair_key(pairs[1]), "unsure", ""),
    }
    prec, n = precision_at(pairs, labels, t=0.6, require_gate=True)
    assert n == 2 and prec == 0.5


def test_recall_by_kind_splits_retrieved_from_missed():
    # o1|p1 is above the threshold, o2|p2 is below it, so the two kinds must not be averaged.
    pairs = [cp("o1", "p1", jaccard=0.8), cp("o2", "p2", jaccard=0.4)]
    planted = [Planted("o1", "p1", "rename"), Planted("o2", "p2", "insert")]
    assert recall_by_kind(pairs, planted, t=0.6, require_gate=True) == {"rename": 1.0, "insert": 0.0}


def test_sweep_and_choose():
    pairs = [cp("o1", "p1", jaccard=0.8), cp("o2", "p2", jaccard=0.5)]
    planted = [Planted("o1", "p1", "rename"), Planted("o2", "p2", "insert")]
    labels = {pair_key(pairs[0]): Label(pair_key(pairs[0]), "dup", "")}
    rows = sweep(pairs, planted, pairs, labels, require_gate=True)
    assert rows[0]["t"] == 0.3 and rows[-1]["t"] == 0.95
    chosen = choose(rows, target_recall=0.9)
    assert chosen is not None and chosen["t"] == 0.5 and chosen["recall"] == 1.0
