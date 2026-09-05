import sys
from datetime import date

from fp.index import CandidatePair, read_jsonl
from fp.label import Label, pair_key, read_labels
from fp.mutate import Planted, read_planted

THRESHOLDS = [round(0.30 + 0.05 * i, 2) for i in range(14)]  # 0.30 .. 0.95


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


def write_report(path: str, rows_gate: list[dict], rows_nogate: list[dict], chosen_gate: dict | None,
                 chosen_nogate: dict | None, counts: dict, kind_recall: dict[str, float] | None = None) -> None:
    def headline(c: dict | None) -> str:
        if c is None:
            return "recall target not reached at any threshold"
        return f"precision {c['precision']:.2f} at t={c['t']:.2f} (recall {c['recall']:.2f}, {c['labelled_predicted']} labelled pairs)"

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
    chosen_t = f"{chosen_gate['t']:.2f}" if chosen_gate else "n/a"
    body = f"""# Fingerprinting spike report ({date.today().isoformat()})

## Headline
With signature gate: {headline(chosen_gate)}
Without signature gate: {headline(chosen_nogate)}

**Verdict: {verdict}**

## Recall by mutation kind, gate on, t={chosen_t}
{_kind_table(kind_recall)}

## Definitions
- Recall at t: fraction of {counts['planted']} planted near-duplicates retrieved at threshold t.
- Precision at t: among labelled candidate pairs predicted at t, fraction labelled dup. Unsure labels excluded.
- Headline: precision at the highest t with recall >= 0.90, gate on.
- Signals: structural hash (always predicts), MinHash Jaccard over 5-token shingles with 128 permutations, signature gate (param count equal or callee Jaccard >= 0.5).
- Caveat, planted recall is optimistic: recall is measured against planted mutations, which are mechanical and far simpler than real-world divergence, so treat these numbers as an upper bound. The literals slot is additionally biased toward functions that contain string literals, because a mutation that would not change the source is skipped rather than planted.
- Caveat, signature statistics on planted pairs are conservative: the rename mutation is scope-blind and rewrites every word-boundary match, so it can rename a property name as well as a local and depress the measured signature similarity. Candidate retrieval is unaffected, because the filter that produces these pairs uses the structural hash and the Jaccard floor only; the gate-on recall column can still lose a planted pair whose signature similarity was depressed, so compare it with the gate-off sweep.

## Counts
functions_in_pairs={counts['functions']} candidate_pairs={counts['pairs']} labelled={counts['labelled']} planted={counts['planted']}

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
    with open(path, "w", encoding="utf8") as f:
        f.write(body)


if __name__ == "__main__":
    candidates_real, candidates_planted, planted_path, labels_path, out = sys.argv[1:6]
    pairs_real = read_jsonl(candidates_real)
    pairs_planted = read_jsonl(candidates_planted)
    planted = read_planted(planted_path)
    labels = read_labels(labels_path)
    rows_gate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=True)
    rows_nogate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=False)
    cg, cn = choose(rows_gate), choose(rows_nogate)
    kinds = recall_by_kind(pairs_planted, planted, cg["t"], require_gate=True) if cg else None
    functions = len({i for p in pairs_real for i in (p.a_id, p.b_id)})
    write_report(out, rows_gate, rows_nogate, cg, cn,
                 {"functions": functions, "pairs": len(pairs_real), "labelled": len(labels), "planted": len(planted)},
                 kind_recall=kinds)
    with open(out, encoding="utf8") as f:
        print(f.read().split("## Definitions")[0])
