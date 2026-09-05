from fp.evaluate import (BUCKET_HIGH, BUCKET_MID, BUCKET_STRUCTURAL, bucket_counts, choose,
                         label_breakdown, precision_at, predicted, recall_at, recall_by_kind,
                         sweep, weighted_precision, write_report)
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


# --- population weighting, gate honesty, label breakdown -------------------------------

def strata_fixture():
    """Four structural pairs, six high jaccard pairs, two mid jaccard pairs.

    Labels cover 2 of the structural pairs (both dup) and 4 of the high pairs (1 dup,
    3 not), so pooled precision over labelled predicted pairs is 3/6 = 0.50 while the
    population weighted average is (1.00 * 4 + 0.25 * 6) / 10 = 0.55.
    """
    structural = [cp(f"s{i}a", f"s{i}b", structural=True) for i in range(4)]
    high = [cp(f"h{i}a", f"h{i}b", jaccard=0.8) for i in range(6)]
    mid = [cp(f"m{i}a", f"m{i}b", jaccard=0.4) for i in range(2)]
    pairs = structural + high + mid
    labels = {}
    for p in structural[:2]:
        labels[pair_key(p)] = Label(pair_key(p), "dup", "")
    labels[pair_key(high[0])] = Label(pair_key(high[0]), "dup", "")
    for p in high[1:4]:
        labels[pair_key(p)] = Label(pair_key(p), "not", "")
    # An unsure label must not count in either the numerator or the denominator.
    labels[pair_key(high[4])] = Label(pair_key(high[4]), "unsure", "")
    return pairs, labels


def test_bucket_counts_sizes_the_three_labelling_buckets():
    pairs, _ = strata_fixture()
    assert bucket_counts(pairs) == {BUCKET_STRUCTURAL: 4, BUCKET_HIGH: 6, BUCKET_MID: 2}


def test_bucket_counts_ignores_pairs_below_the_sampling_floor():
    # A non-structural pair under jaccard 0.30 belongs to no labelling bucket.
    pairs = [cp("a", "b", jaccard=0.1), cp("c", "d", structural=True)]
    assert bucket_counts(pairs) == {BUCKET_STRUCTURAL: 1, BUCKET_HIGH: 0, BUCKET_MID: 0}


def test_weighted_precision_differs_from_sample_pooled_precision():
    pairs, labels = strata_fixture()
    pooled, n = precision_at(pairs, labels, t=0.6, require_gate=True)
    assert n == 6 and pooled == 0.5
    # The structural stratum is 4 of the 10 predicted pairs, not 2 of the 6 labelled ones.
    assert weighted_precision(pairs, labels, t=0.6, require_gate=True) == 0.55


def test_weighted_precision_ignores_strata_with_no_usable_label():
    # Only the structural stratum carries a label, so it alone sets the number.
    pairs = [cp("s1a", "s1b", structural=True), cp("s2a", "s2b", structural=True),
             cp("h1a", "h1b", jaccard=0.8)]
    labels = {pair_key(pairs[0]): Label(pair_key(pairs[0]), "dup", "")}
    assert weighted_precision(pairs, labels, t=0.6, require_gate=True) == 1.0


def test_weighted_precision_is_none_without_any_labelled_prediction():
    pairs, _ = strata_fixture()
    assert weighted_precision(pairs, {}, t=0.6, require_gate=True) is None


def test_label_breakdown_counts_dup_not_unsure_per_bucket():
    pairs, labels = strata_fixture()
    assert label_breakdown(pairs, labels) == {
        BUCKET_STRUCTURAL: {"dup": 2, "not": 0, "unsure": 0},
        BUCKET_HIGH: {"dup": 1, "not": 3, "unsure": 1},
        BUCKET_MID: {"dup": 0, "not": 0, "unsure": 0},
    }


def test_write_report_renders_weighting_breakdown_and_bucket_populations(tmp_path):
    rows = [row(0.30, 1.0, 0.60, 20), row(0.50, 1.0, 0.90, 12), row(0.95, 1.0, 0.0, 0)]
    chosen = rows[1]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, chosen, chosen, COUNTS, kind_recall={"rename": 1.0},
                 bucket_pop={BUCKET_STRUCTURAL: 4, BUCKET_HIGH: 6, BUCKET_MID: 2},
                 weighted=0.55,
                 breakdown={BUCKET_STRUCTURAL: {"dup": 2, "not": 0, "unsure": 0},
                            BUCKET_HIGH: {"dup": 1, "not": 3, "unsure": 1},
                            BUCKET_MID: {"dup": 0, "not": 0, "unsure": 0}})
    text = open(out, encoding="utf8").read()
    assert "Population-weighted precision at the chosen threshold: 0.55 (sample-pooled 0.90)" in text
    head, _, rest = text.partition("Population-weighted precision at the chosen threshold")
    # The line sits directly under the two headline lines, above the verdict.
    assert head.rstrip().endswith("labelled pairs predicted at t)")
    assert rest.lstrip().startswith(": 0.55 (sample-pooled 0.90)\n\n**Verdict:")
    assert f"{BUCKET_STRUCTURAL}=4" in text and f"{BUCKET_HIGH}=6" in text and f"{BUCKET_MID}=2" in text
    assert "## Label breakdown by sample bucket" in text
    assert f"| {BUCKET_STRUCTURAL} | 2 | 0 | 0 | 1.00 |" in text
    assert f"| {BUCKET_HIGH} | 1 | 3 | 1 | 0.25 |" in text
    # A bucket with nothing labelled has no precision, and must not claim zero.
    assert f"| {BUCKET_MID} | 0 | 0 | 0 | n/a |" in text


def test_write_report_headline_says_the_count_is_pairs_predicted_at_t(tmp_path):
    rows = [row(0.50, 1.0, 0.90, 12)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, rows[0], rows[0], COUNTS)
    text = open(out, encoding="utf8").read()
    assert "(recall 1.00, 12 labelled pairs predicted at t)" in text
    assert "12 labelled pairs)" not in text


def test_write_report_marks_the_signature_gate_untested_on_a_tie(tmp_path):
    rows = [row(0.50, 1.0, 0.90, 12)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, rows[0], dict(rows[0]), COUNTS)
    text = open(out, encoding="utf8").read()
    assert ('signature_gate = on (untested: the gate removed no labelled pair above '
            't=0.40 on this sample; "on" is the tie rule)') in text


def test_write_report_keeps_a_bare_on_when_the_gate_actually_helped(tmp_path):
    rows_gate = [row(0.50, 1.0, 0.90, 12)]
    rows_nogate = [row(0.50, 1.0, 0.70, 18)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows_gate, rows_nogate, rows_gate[0], rows_nogate[0], COUNTS)
    text = open(out, encoding="utf8").read()
    assert "signature_gate = on\n" in text
    assert "untested" not in text


def test_write_report_carries_the_status_line_and_hand_written_sections(tmp_path):
    rows = [row(0.50, 1.0, 0.90, 12)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, rows[0], rows[0], COUNTS,
                 status_line="Status: PROVISIONAL. Spot-check pending.",
                 extra_sections="## Tuning pass\nNot run.\n")
    text = open(out, encoding="utf8").read()
    lines = text.splitlines()
    assert lines[0].startswith("# Fingerprinting spike report")
    assert lines[1] == "Status: PROVISIONAL. Spot-check pending."
    assert text.rstrip().endswith("## Tuning pass\nNot run.")


def test_write_report_omits_the_status_line_when_none_is_given(tmp_path):
    rows = [row(0.50, 1.0, 0.90, 12)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, rows[0], rows[0], COUNTS)
    lines = open(out, encoding="utf8").read().splitlines()
    assert lines[0].startswith("# Fingerprinting spike report") and lines[1] == ""


def test_write_report_states_the_labelling_tie_breaker_and_known_hash_collisions(tmp_path):
    rows = [row(0.50, 1.0, 0.90, 12)]
    out = str(tmp_path / "REPORT.md")
    write_report(out, rows, rows, rows[0], rows[0], COUNTS)
    text = open(out, encoding="utf8").read()
    assert "one-line wrapper over a different constant, endpoint, or table" in text
    assert "the 50-pair spot-check contains none of the first class" in text
    assert "template_string collapses to one LIT placeholder in the structural hash" in text
    assert "honest combined recall is 16 of 23 at the chosen threshold" in text
