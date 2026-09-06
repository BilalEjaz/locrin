from datasketch import MinHash, MinHashLSH

SHINGLE_K = 5
NUM_PERM = 128


def shingles(toks: list[str], k: int = SHINGLE_K) -> set[str]:
    if len(toks) < k:
        return {" ".join(toks)} if toks else set()
    return {" ".join(toks[i:i + k]) for i in range(len(toks) - k + 1)}


def minhash_of(toks: list[str]) -> MinHash:
    m = MinHash(num_perm=NUM_PERM)
    for s in shingles(toks):
        m.update(s.encode("utf8"))
    return m


def estimated_jaccard(a: MinHash, b: MinHash) -> float:
    return float(a.jaccard(b))


class LshIndex:
    def __init__(self, threshold: float):
        self._lsh = MinHashLSH(threshold=threshold, num_perm=NUM_PERM)

    def add(self, key: str, m: MinHash) -> None:
        self._lsh.insert(key, m)

    def query(self, m: MinHash) -> list[str]:
        return list(self._lsh.query(m))
