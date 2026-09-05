import json
import random
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

from fp.index import CandidatePair, read_jsonl


@dataclass
class Label:
    pair_key: str
    label: str
    reason: str


def pair_key(p: CandidatePair) -> str:
    return "|".join(sorted((p.a_id, p.b_id)))


def read_labels(path: str) -> dict[str, Label]:
    if not Path(path).exists():
        return {}
    out: dict[str, Label] = {}
    with open(path, encoding="utf8") as f:
        for line in f:
            if line.strip():
                l = Label(**json.loads(line))
                out[l.pair_key] = l
    return out


def append_label(path: str, label: Label) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a", encoding="utf8") as f:
        f.write(json.dumps(asdict(label)) + "\n")


def sample_for_labelling(pairs: list[CandidatePair], n: int, seed: int) -> list[CandidatePair]:
    rng = random.Random(seed)
    structural = [p for p in pairs if p.structural_match]
    high = [p for p in pairs if not p.structural_match and p.jaccard >= 0.6]
    mid = [p for p in pairs if not p.structural_match and 0.3 <= p.jaccard < 0.6]
    per = max(1, n // 3)
    picked: list[CandidatePair] = []
    for bucket in (structural, high, mid):
        picked.extend(rng.sample(bucket, k=min(per, len(bucket))))
    rng.shuffle(picked)
    return picked


def _show(p: CandidatePair) -> None:
    print("=" * 100)
    print(f"A {p.a_file}  {p.a_name}")
    print(f"B {p.b_file}  {p.b_name}")
    print(f"structural={p.structural_match} jaccard={p.jaccard:.2f} sig_sim={p.sig_sim:.2f} gate={p.sig_gate}")
    print("-" * 48 + " A " + "-" * 48)
    print(p.a_source[:2500])
    print("-" * 48 + " B " + "-" * 48)
    print(p.b_source[:2500])


if __name__ == "__main__":
    candidates_path, labels_path, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    pairs = read_jsonl(candidates_path)
    done = read_labels(labels_path)
    todo = [p for p in sample_for_labelling(pairs, n, seed=7) if pair_key(p) not in done]
    print(f"{len(todo)} pairs to label. d=dup n=not u=unsure q=quit. Add a reason after a space.")
    for p in todo:
        _show(p)
        raw = input("> ").strip()
        if raw.startswith("q"):
            break
        code, _, reason = raw.partition(" ")
        label = {"d": "dup", "n": "not", "u": "unsure"}.get(code)
        if label is None:
            print("skipped")
            continue
        append_label(labels_path, Label(pair_key(p), label, reason))
    print(f"labels now: {len(read_labels(labels_path))}")
