import json
import sys
from dataclasses import asdict, dataclass
from itertools import combinations, islice
from pathlib import Path
from typing import Iterable, Iterator

from datasketch import MinHash

from fp.extract import FunctionRecord, extract_functions
from fp.minhash import LshIndex, estimated_jaccard, minhash_of
from fp.normalize import MIN_TOKENS, structural_hash, tokens
from fp.signature import Signature, gate, signature_of, similarity

EXCLUDE_DIRS = {"node_modules", "android", "ios", ".expo", "dist", "build", "coverage",
                "__mocks__", ".git", ".venv", "__tests__", "e2e", "test", "tests"}
SOURCE_SUFFIXES = {".ts", ".tsx"}
TEST_FILE_MARKERS = (".test.", ".spec.")


def iter_source_files(roots: list[str]) -> Iterator[str]:
    for root in roots:
        root_path = Path(root)
        for p in root_path.rglob("*"):
            if not p.is_file() or p.suffix not in SOURCE_SUFFIXES or p.name.endswith(".d.ts"):
                continue
            if any(marker in p.name for marker in TEST_FILE_MARKERS):
                continue
            # Match against the parts below the root only. Matching p.parts would let an
            # ancestor of the root veto everything under it, which is how the fixture repo
            # (itself under tests/) and any repo checked out under build/ or dist/ break.
            if any(part in EXCLUDE_DIRS for part in p.relative_to(root_path).parts[:-1]):
                continue
            yield str(p.resolve()).replace("\\", "/")


@dataclass
class Indexed:
    record: FunctionRecord
    toks: list[str]
    shash: str
    mh: MinHash
    sig: Signature


def build_files(files: Iterable[str]) -> list[Indexed]:
    items: list[Indexed] = []
    for file in files:
        source = Path(file).read_text(encoding="utf8", errors="replace")
        for rec in extract_functions(file, source):
            toks = tokens(rec.source)
            if len(toks) < MIN_TOKENS:
                continue
            items.append(Indexed(rec, toks, structural_hash(toks), minhash_of(toks), signature_of(rec.source)))
    return items


def build(roots: list[str]) -> list[Indexed]:
    return build_files(iter_source_files(roots))


@dataclass
class CandidatePair:
    a_id: str
    b_id: str
    a_file: str
    b_file: str
    a_name: str
    b_name: str
    structural_match: bool
    jaccard: float
    sig_sim: float
    sig_gate: bool
    a_source: str
    b_source: str


def _pair(a: Indexed, b: Indexed) -> CandidatePair:
    return CandidatePair(
        a_id=a.record.id, b_id=b.record.id,
        a_file=a.record.file, b_file=b.record.file,
        a_name=a.record.name, b_name=b.record.name,
        structural_match=a.shash == b.shash,
        jaccard=estimated_jaccard(a.mh, b.mh),
        sig_sim=similarity(a.sig, b.sig),
        sig_gate=gate(a.sig, b.sig),
        a_source=a.record.source, b_source=b.record.source,
    )


def _nested(a: Indexed, b: Indexed) -> bool:
    if a.record.file != b.record.file:
        return False
    return (a.record.start_line <= b.record.start_line <= a.record.end_line) or \
           (b.record.start_line <= a.record.start_line <= b.record.end_line)


def candidate_pairs(items: list[Indexed], floor: float = 0.3) -> list[CandidatePair]:
    by_id = {i.record.id: i for i in items}
    seen: set[frozenset[str]] = set()
    pairs: list[CandidatePair] = []

    by_hash: dict[str, list[Indexed]] = {}
    for i in items:
        by_hash.setdefault(i.shash, []).append(i)
    for group in by_hash.values():
        for a, b in combinations(group, 2):
            key = frozenset((a.record.id, b.record.id))
            if key in seen or _nested(a, b):
                continue
            seen.add(key)
            pairs.append(_pair(a, b))

    lsh = LshIndex(threshold=floor)
    for i in items:
        lsh.add(i.record.id, i.mh)
    for i in items:
        for other_id in lsh.query(i.mh):
            if other_id == i.record.id:
                continue
            key = frozenset((i.record.id, other_id))
            if key in seen:
                continue
            seen.add(key)
            other = by_id[other_id]
            if _nested(i, other):
                continue
            # LSH banding returns matches well below its nominal threshold, so the floor
            # has to be enforced here too. Structural matches are kept whatever the
            # estimate says, but those were already emitted by the loop above.
            if i.shash != other.shash and estimated_jaccard(i.mh, other.mh) < floor:
                continue
            pairs.append(_pair(i, other))
    pairs.sort(key=lambda p: (-int(p.structural_match), -p.jaccard))
    return pairs


def write_jsonl(pairs: list[CandidatePair], path: str) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf8") as f:
        for p in pairs:
            f.write(json.dumps(asdict(p)) + "\n")


def read_jsonl(path: str) -> list[CandidatePair]:
    with open(path, encoding="utf8") as f:
        return [CandidatePair(**json.loads(line)) for line in f if line.strip()]


if __name__ == "__main__":
    argv = sys.argv[1:]
    max_files: int | None = None
    if "--max-files" in argv:
        i = argv.index("--max-files")
        max_files = int(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    *roots, out = argv
    files: Iterable[str] = iter_source_files(roots)
    if max_files is not None:
        files = islice(files, max_files)
    items = build_files(files)
    pairs = candidate_pairs(items)
    write_jsonl(pairs, out)
    print(f"functions={len(items)} pairs={len(pairs)} -> {out}")
