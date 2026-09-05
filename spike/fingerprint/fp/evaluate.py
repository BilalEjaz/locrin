import sys
from datetime import date

from fp.index import CandidatePair, read_jsonl
from fp.label import Label, pair_key, read_labels
from fp.mutate import Planted, read_planted

THRESHOLDS = [round(0.30 + 0.05 * i, 2) for i in range(14)]  # 0.30 .. 0.95

BUCKET_STRUCTURAL = "structural match"
BUCKET_HIGH = "jaccard >= 0.60, no structural match"
BUCKET_MID = "jaccard 0.30 to 0.60, no structural match"
BUCKETS = (BUCKET_STRUCTURAL, BUCKET_HIGH, BUCKET_MID)


def predicted(p: CandidatePair, t: float, require_gate: bool) -> bool:
    if p.structural_match:
        return True
    if p.jaccard < t:
        return False
    return p.sig_gate or not require_gate


def _planted_key(pl: Planted) -> str:
    # Ids are opaque strings, so build the key the same way label.pair_key does rather
    # than parsing either id apart.
    return "|".join(sorted((pl.original_id, pl.planted_id)))


def _hit_keys(pairs: list[CandidatePair], t: float, require_gate: bool) -> set[str]:
    return {pair_key(p) for p in pairs if predicted(p, t, require_gate)}


def recall_at(pairs: list[CandidatePair], planted: list[Planted], t: float, require_gate: bool) -> float:
    if not planted:
        return 0.0
    hit_keys = _hit_keys(pairs, t, require_gate)
    found = sum(1 for pl in planted if _planted_key(pl) in hit_keys)
    return found / len(planted)


def recall_by_kind(pairs: list[CandidatePair], planted: list[Planted], t: float,
                   require_gate: bool) -> dict[str, float]:
    """Recall split per mutation kind. The aggregate hides which mutation the matcher
    struggles with, and that is the part worth carrying into the engine."""
    hit_keys = _hit_keys(pairs, t, require_gate)
    total: dict[str, int] = {}
    found: dict[str, int] = {}
    for pl in planted:
        total[pl.kind] = total.get(pl.kind, 0) + 1
        found[pl.kind] = found.get(pl.kind, 0) + (1 if _planted_key(pl) in hit_keys else 0)
    return {kind: found[kind] / total[kind] for kind in total}


def precision_at(pairs: list[CandidatePair], labels: dict[str, Label], t: float, require_gate: bool) -> tuple[float, int]:
    tp = fp = 0
    for p in pairs:
        if not predicted(p, t, require_gate):
            continue
        l = labels.get(pair_key(p))
        if l is None or l.label == "unsure":
            continue
        if l.label == "dup":
            tp += 1
        else:
            fp += 1
    n = tp + fp
    return (tp / n if n else 0.0), n


def bucket_of(p: CandidatePair) -> str | None:
    """Which labelling stratum a pair sits in. Same split as label.sample_for_labelling, so
    the sample and the population it was drawn from are cut the same way. A non-structural
    pair under the 0.30 floor was never eligible for labelling and belongs to no bucket."""
    if p.structural_match:
        return BUCKET_STRUCTURAL
    if p.jaccard >= 0.6:
        return BUCKET_HIGH
    if p.jaccard >= 0.3:
        return BUCKET_MID
    return None


def bucket_counts(pairs: list[CandidatePair]) -> dict[str, int]:
    """Population size of each labelling bucket over all candidate pairs."""
    counts = {b: 0 for b in BUCKETS}
    for p in pairs:
        b = bucket_of(p)
        if b is not None:
            counts[b] += 1
    return counts


def _stratified(pairs: list[CandidatePair], labels: dict[str, Label], t: float,
                require_gate: bool) -> tuple[dict[str, int], dict[str, int], dict[str, int]]:
    tp = {b: 0 for b in BUCKETS}
    n = {b: 0 for b in BUCKETS}
    pop = {b: 0 for b in BUCKETS}
    for p in pairs:
        b = bucket_of(p)
        if b is None or not predicted(p, t, require_gate):
            continue
        pop[b] += 1
        l = labels.get(pair_key(p))
        if l is None or l.label == "unsure":
            continue
        n[b] += 1
        if l.label == "dup":
            tp[b] += 1
    return tp, n, pop


def weighted_precision(pairs: list[CandidatePair], labels: dict[str, Label], t: float,
                       require_gate: bool) -> float | None:
    """Precision over the candidate stream rather than over the sample.

    The sample is stratified, not proportional: the three buckets were labelled in equal
    thirds while the population behind them is nothing like equal. Pooling the labels
    therefore reports the precision of the sample, not of what the engine would emit. Each
    stratum's precision is weighted by how many pairs that stratum contributes to the
    predictions at t. Strata with no usable label are dropped from both sides rather than
    counted as zero, so this is a weighted average over what was actually measured."""
    tp, n, pop = _stratified(pairs, labels, t, require_gate)
    usable = [b for b in BUCKETS if n[b]]
    total = sum(pop[b] for b in usable)
    if not total:
        return None
    return sum((tp[b] / n[b]) * pop[b] for b in usable) / total


def label_breakdown(pairs: list[CandidatePair], labels: dict[str, Label]) -> dict[str, dict[str, int]]:
    """Label counts per bucket. Regenerating the report must not lose this table, because
    it is the only place the reader can see which bucket carries the headline."""
    out = {b: {"dup": 0, "not": 0, "unsure": 0} for b in BUCKETS}
    for p in pairs:
        b = bucket_of(p)
        if b is None:
            continue
        l = labels.get(pair_key(p))
        if l is not None and l.label in out[b]:
            out[b][l.label] += 1
    return out


def sweep(pairs_planted: list[CandidatePair], planted: list[Planted], pairs_real: list[CandidatePair],
          labels: dict[str, Label], require_gate: bool) -> list[dict]:
    rows = []
    for t in THRESHOLDS:
        prec, n = precision_at(pairs_real, labels, t, require_gate)
        rows.append({"t": t, "recall": recall_at(pairs_planted, planted, t, require_gate),
                     "precision": prec, "labelled_predicted": n})
    return rows


def choose(rows: list[dict], target_recall: float = 0.9) -> dict | None:
    ok = [r for r in rows if r["recall"] >= target_recall]
    return max(ok, key=lambda r: r["t"]) if ok else None


def _table(rows: list[dict]) -> str:
    lines = ["| t | recall | precision | labelled predicted |", "|---|---|---|---|"]
    for r in rows:
        # An empty denominator is not a precision of zero, so do not print one.
        prec = "n/a" if r["labelled_predicted"] == 0 else f"{r['precision']:.2f}"
        lines.append(f"| {r['t']:.2f} | {r['recall']:.2f} | {prec} | {r['labelled_predicted']} |")
    return "\n".join(lines)


def _kind_table(kind_recall: dict[str, float] | None) -> str:
    if not kind_recall:
        return "No chosen threshold, so there is no per-kind recall to report."
    lines = ["| mutation kind | recall |", "|---|---|"]
    for kind, value in kind_recall.items():
        lines.append(f"| {kind} | {value:.2f} |")
    return "\n".join(lines)


def _breakdown_table(breakdown: dict[str, dict[str, int]]) -> str:
    lines = ["| bucket | dup | not | unsure | precision |", "|---|---|---|---|---|"]
    for bucket in BUCKETS:
        c = breakdown.get(bucket, {"dup": 0, "not": 0, "unsure": 0})
        decided = c["dup"] + c["not"]
        # Nothing labelled in a bucket is absence of evidence, not a precision of zero.
        prec = f"{c['dup'] / decided:.2f}" if decided else "n/a"
        lines.append(f"| {bucket} | {c['dup']} | {c['not']} | {c['unsure']} | {prec} |")
    return "\n".join(lines)


def _population_lines(bucket_pop: dict[str, int] | None) -> str:
    if not bucket_pop:
        return ""
    lines = ["bucket populations (all candidate pairs, not the 240 sampled):"]
    lines += [f"- {bucket}={bucket_pop.get(bucket, 0)}" for bucket in BUCKETS]
    return "\n" + "\n".join(lines)


def write_report(path: str, rows_gate: list[dict], rows_nogate: list[dict], chosen_gate: dict | None,
                 chosen_nogate: dict | None, counts: dict, kind_recall: dict[str, float] | None = None,
                 bucket_pop: dict[str, int] | None = None, weighted: float | None = None,
                 breakdown: dict[str, dict[str, int]] | None = None,
                 status_line: str | None = None, extra_sections: str = "") -> None:
    def headline(c: dict | None) -> str:
        if c is None:
            return "recall target not reached at any threshold"
        return (f"precision {c['precision']:.2f} at t={c['t']:.2f} (recall {c['recall']:.2f}, "
                f"{c['labelled_predicted']} labelled pairs predicted at t)")

    # A precision of 0.0 on an empty denominator means nothing labelled was predicted at
    # this threshold. That is absence of evidence, not evidence of bad precision, so it
    # must not decide the verdict or the gate recommendation.
    gate_undetermined = chosen_gate is not None and chosen_gate["labelled_predicted"] == 0
    verdict = "UNKNOWN"
    if gate_undetermined:
        verdict = "UNKNOWN: no labelled pairs predicted at the chosen threshold"
    elif chosen_gate is not None:
        verdict = "PASS: ship already-exists in version one" if chosen_gate["precision"] >= 0.85 \
            else "FAIL: move already-exists to release two"
    nogate_precision = chosen_nogate["precision"] if chosen_nogate else 0.0
    gate_choice = "undetermined" if gate_undetermined else \
        ("on" if chosen_gate and chosen_gate["precision"] >= nogate_precision else "off")
    # A tie is not evidence for the gate. Say so where the number would be copied out.
    tied = (not gate_undetermined and chosen_gate is not None and chosen_nogate is not None
            and chosen_gate["recall"] == chosen_nogate["recall"]
            and chosen_gate["precision"] == chosen_nogate["precision"])
    if tied:
        gate_choice = ('on (untested: the gate removed no labelled pair above t=0.40 on this '
                       'sample; "on" is the tie rule)')
    chosen_t = f"{chosen_gate['t']:.2f}" if chosen_gate else "n/a"
    pooled = "n/a" if (chosen_gate is None or chosen_gate["labelled_predicted"] == 0) \
        else f"{chosen_gate['precision']:.2f}"
    weighted_text = "n/a" if weighted is None else f"{weighted:.2f}"
    title_block = f"# Fingerprinting spike report ({date.today().isoformat()})"
    if status_line:
        title_block += f"\n{status_line}"
    body = f"""{title_block}

## Headline
With signature gate: {headline(chosen_gate)}
Without signature gate: {headline(chosen_nogate)}
Population-weighted precision at the chosen threshold: {weighted_text} (sample-pooled {pooled})

**Verdict: {verdict}**

## Recall by mutation kind, gate on, t={chosen_t}
{_kind_table(kind_recall)}

## Definitions
- Recall at t: fraction of {counts['planted']} planted near-duplicates retrieved at threshold t.
- Precision at t: among labelled candidate pairs predicted at t, fraction labelled dup. Unsure labels excluded.
- Headline: precision at the highest t with recall >= 0.90, gate on. The pair count on that line is the labelled pairs predicted at t, not the size of the label set.
- Population-weighted precision: the sample is stratified in equal thirds across three buckets whose populations are nothing like equal, so the pooled number is the precision of the sample. The weighted number reweights each bucket's precision by that bucket's share of the pairs predicted at t, which is the closer estimate of what the engine would emit.
- Labelling tie-breaker actually applied: Same shape with one differing callee was labelled dup when the shared body is multi-line and not when it is a one-line wrapper over a different constant, endpoint, or table; the 50-pair spot-check contains none of the first class.
- Signals: structural hash (always predicts), MinHash Jaccard over 5-token shingles with 128 permutations, signature gate (param count equal or callee Jaccard >= 0.5).
- Caveat, planted recall is optimistic: recall is measured against planted mutations, which are mechanical and far simpler than real-world divergence, so treat these numbers as an upper bound. The literals slot is additionally biased toward functions that contain string literals, because a mutation that would not change the source is skipped rather than planted.
- Caveat, signature statistics on planted pairs are conservative: the rename mutation is scope-blind and rewrites every word-boundary match, so it can rename a property name as well as a local and depress the measured signature similarity. Candidate retrieval is unaffected, because the filter that produces these pairs uses the structural hash and the Jaccard floor only; the gate-on recall column can still lose a planted pair whose signature similarity was depressed, so compare it with the gate-off sweep.
- Caveat, template literals collide: template_string collapses to one LIT placeholder in the structural hash, so two functions differing only inside template substitutions hash equal; this is a known source of structural false positives.
- Caveat, two combined plants are free hits: Two combined plants no-op'd their insert step and are structural matches, so honest combined recall is 16 of 23 at the chosen threshold.

## Counts
functions_in_pairs={counts['functions']} candidate_pairs={counts['pairs']} labelled={counts['labelled']} planted={counts['planted']}{_population_lines(bucket_pop)}

## Sweep, gate on
{_table(rows_gate)}

## Sweep, gate off
{_table(rows_nogate)}

## Thresholds to carry into the engine (spec section 3.3)
- SHINGLE_K = 5, NUM_PERM = 128
- jaccard_threshold = {chosen_t}
- signature_gate = {gate_choice}
- MIN_TOKENS = 40
"""
    if breakdown is not None:
        body += f"\n## Label breakdown by sample bucket\n{_breakdown_table(breakdown)}\n"
    if extra_sections:
        body += "\n" + extra_sections.strip("\n") + "\n"
    with open(path, "w", encoding="utf8") as f:
        f.write(body)


def _parse_argv(argv: list[str]) -> tuple[list[str], str | None, str]:
    """Positional arguments keep their old order and meaning. The two optional flags carry
    the hand-written parts of the report (the status line and any hand-written sections)
    through a regeneration, so re-running this command cannot silently drop them."""
    positional: list[str] = []
    status_line: str | None = None
    extra = ""
    i = 0
    while i < len(argv):
        if argv[i] == "--status":
            status_line = argv[i + 1]
            i += 2
        elif argv[i] == "--extra":
            with open(argv[i + 1], encoding="utf8") as f:
                extra = f.read()
            i += 2
        else:
            positional.append(argv[i])
            i += 1
    return positional, status_line, extra


if __name__ == "__main__":
    _positional, _status, _extra = _parse_argv(sys.argv[1:])
    candidates_real, candidates_planted, planted_path, labels_path, out = _positional[:5]
    pairs_real = read_jsonl(candidates_real)
    pairs_planted = read_jsonl(candidates_planted)
    planted = read_planted(planted_path)
    labels = read_labels(labels_path)
    rows_gate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=True)
    rows_nogate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=False)
    cg, cn = choose(rows_gate), choose(rows_nogate)
    kinds = recall_by_kind(pairs_planted, planted, cg["t"], require_gate=True) if cg else None
    functions = len({i for p in pairs_real for i in (p.a_id, p.b_id)})
    weighted = weighted_precision(pairs_real, labels, cg["t"], require_gate=True) if cg else None
    write_report(out, rows_gate, rows_nogate, cg, cn,
                 {"functions": functions, "pairs": len(pairs_real), "labelled": len(labels), "planted": len(planted)},
                 kind_recall=kinds, bucket_pop=bucket_counts(pairs_real), weighted=weighted,
                 breakdown=label_breakdown(pairs_real, labels),
                 status_line=_status, extra_sections=_extra)
    with open(out, encoding="utf8") as f:
        print(f.read().split("## Definitions")[0])
