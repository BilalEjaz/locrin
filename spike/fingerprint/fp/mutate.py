import json
import random
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable

from fp.extract import FunctionRecord
from fp.index import Indexed, build, candidate_pairs, write_jsonl
from fp.minhash import minhash_of
from fp.normalize import structural_hash, tokens
from fp.signature import signature_of

RESERVED = {"function", "async", "await", "const", "let", "var", "return", "if", "else", "for",
            "of", "in", "while", "throw", "new", "true", "false", "null", "undefined", "string",
            "number", "boolean", "void", "export", "default", "class", "this", "import", "from",
            "try", "catch", "finally", "switch", "case", "break", "continue", "typeof", "instanceof"}

_IDENT = re.compile(r"\b([A-Za-z_$][A-Za-z0-9_$]*)\b")
_STRING = re.compile(r'"([^"\\]|\\.)*"|\'([^\'\\]|\\.)*\'')


def _rename(source: str, rng: random.Random) -> str:
    names = [n for n in dict.fromkeys(_IDENT.findall(source)) if n not in RESERVED and len(n) > 1]
    targets = rng.sample(names, k=min(3, len(names))) if names else []
    out = source
    for i, name in enumerate(targets):
        out = re.sub(rf"\b{re.escape(name)}\b", f"{name}Alt{i}", out)
    return out


def _insert(source: str, rng: random.Random) -> str:
    lines = source.split("\n")
    candidates = [i for i, l in enumerate(lines) if l.rstrip().endswith("{")]
    if not candidates:
        return source
    at = rng.choice(candidates)
    indent = re.match(r"\s*", lines[at]).group(0) + "  "
    lines.insert(at + 1, f'{indent}console.log("planted");')
    return "\n".join(lines)


def _literals(source: str, rng: random.Random) -> str:
    counter = [0]

    def repl(m: re.Match) -> str:
        counter[0] += 1
        return f'"planted-{rng.randint(0, 9999)}-{counter[0]}"'

    return _STRING.sub(repl, source)


def _combined(source: str, rng: random.Random) -> str:
    return _insert(_literals(_rename(source, rng), rng), rng)


MUTATIONS: dict[str, Callable[[str, random.Random], str]] = {
    "rename": _rename,
    "insert": _insert,
    "literals": _literals,
    "combined": _combined,
}


def mutate(source: str, kind: str, seed: int) -> str:
    return MUTATIONS[kind](source, random.Random(seed))


@dataclass
class Planted:
    original_id: str
    planted_id: str
    kind: str


def plant(items: list[Indexed], n: int, seed: int) -> tuple[list[Indexed], list[Planted]]:
    rng = random.Random(seed)
    chosen = rng.sample(items, k=min(n, len(items)))
    kinds = list(MUTATIONS)
    extended = list(items)
    planted: list[Planted] = []
    for k, item in enumerate(chosen):
        kind = kinds[k % len(kinds)]
        new_src = mutate(item.record.source, kind, seed=seed + k)
        rec = item.record
        planted_file = f"{rec.file}__planted_{k}.ts"
        new_rec = FunctionRecord(
            id=f"{planted_file}:{rec.start_line}:{rec.name}Planted",
            file=planted_file, name=f"{rec.name}Planted",
            start_line=rec.start_line, end_line=rec.end_line, source=new_src,
        )
        toks = tokens(new_src)
        extended.append(Indexed(new_rec, toks, structural_hash(toks), minhash_of(toks), signature_of(new_src)))
        planted.append(Planted(rec.id, new_rec.id, kind))
    return extended, planted


def write_planted(planted: list[Planted], path: str) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf8") as f:
        for p in planted:
            f.write(json.dumps(asdict(p)) + "\n")


def read_planted(path: str) -> list[Planted]:
    with open(path, encoding="utf8") as f:
        return [Planted(**json.loads(line)) for line in f if line.strip()]


if __name__ == "__main__":
    *roots, n, pairs_out, planted_out = sys.argv[1:]
    items = build(roots)
    extended, planted = plant(items, n=int(n), seed=42)
    write_jsonl(candidate_pairs(extended), pairs_out)
    write_planted(planted, planted_out)
    print(f"functions={len(items)} planted={len(planted)} -> {pairs_out}, {planted_out}")
