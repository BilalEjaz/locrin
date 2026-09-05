from fp.evaluate import choose, precision_at, predicted, recall_at, recall_by_kind, sweep, write_report
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


def row(t, recall, precision, labelled):
    return {"t": t, "recall": recall, "precision": precision, "labelled_predicted": labelled}


COUNTS = {"functions": 6, "pairs": 3, "labelled": 2, "planted": 2}


def test_write_report_renders_kinds_caveats_and_verdict(tmp_path):
    rows = [row(0.30, 1.0, 0.60, 20), row(0.50, 1.0, 0.90, 12), row(0.95, 1.0, 0.0, 0)]
    chosen = rows[1]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, chosen, chosen, COUNTS, kind_recall={"rename": 1.0, "insert": 0.5})
    text = out and open(out, encoding="utf8").read()
    assert "## Recall by mutation kind, gate on, t=0.50" in text
    assert "| rename | 1.00 |" in text and "| insert | 0.50 |" in text
    # A distinctive phrase from each of the two honest caveats.
    assert "mechanical and far simpler than real-world divergence" in text
    assert "Candidate retrieval is unaffected" in text
    assert "compare it with the gate-off sweep" in text
    assert "**Verdict: PASS: ship already-exists in version one**" in text
    # The count is functions seen in pairs, not the corpus size, and must say so.
    assert "functions_in_pairs=6" in text
    # An empty labelled denominator is not a precision of zero.
    assert "| 0.95 | 1.00 | n/a | 0 |" in text
    assert "| 0.50 | 1.00 | 0.90 | 12 |" in text


def test_write_report_survives_no_chosen_threshold(tmp_path):
    rows = [row(0.30, 0.2, 0.5, 4)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, None, None, COUNTS, kind_recall=None)
    text = open(out, encoding="utf8").read()
    assert text.count("recall target not reached at any threshold") == 2
    assert "**Verdict: UNKNOWN**" in text
    assert "jaccard_threshold = n/a" in text


def test_write_report_verdict_is_unknown_when_no_labelled_pairs(tmp_path):
    # precision 0.0 here means "nothing labelled was predicted", not "all false positives".
    rows = [row(0.30, 1.0, 0.0, 0)]
    chosen = rows[0]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, chosen, chosen, COUNTS, kind_recall={"rename": 1.0})
    text = open(out, encoding="utf8").read()
    assert "**Verdict: UNKNOWN: no labelled pairs predicted at the chosen threshold**" in text
    assert "signature_gate = undetermined" in text
