# Fingerprinting spike (throwaway)

Measures whether structural hash + MinHash + signature vector finds near-duplicate
TypeScript functions at >= 85% precision at 90% recall on the founder's repos.

Run: `.venv/Scripts/python -m pytest` for tests. Pipeline commands are in
`docs/superpowers/plans/2026-09-05-fingerprint-spike.md`. The result lives in `REPORT.md`.

Nothing in here ships. The production engine is Rust.

## Run log

All six roots (`fasting-app/src`, `fasting-app/app`, `fasting-app/components`,
`strongspan/src`, `food-data-platform/apps`, `food-data-platform/packages`):

```
functions=20453 pairs=16831193
real    10m38.383s
```

That run crossed the ten minute budget and wrote a 28.8 GB `data/candidates.jsonl`,
so `--max-files` was added to `__main__` and the run was repeated on `fasting-app/src`
alone:

```
functions=14665 pairs=9714889
real    6m14.600s
```

After both plan defects were fixed (LSH pairs filtered by the jaccard floor, test code
excluded from the corpus), all six roots again:

```
functions=4327 pairs=32524
real    0m24.473s
data/candidates.jsonl  96 MB
```

That is 517x fewer pairs, 26x faster and 300x smaller than the first six-root run, and
comfortably inside the ten minute budget. The corpus shrank from 20453 functions to
4327 because 960 of the 1904 source files were test code: 958 matched `.test.` or
`.spec.` in the file name and 2 sat under a `test` directory.

Planted run (`fp.mutate`, same six roots, n=100, seed=42):

```
functions=4354 planted=100 -> data/candidates_planted.jsonl, data/planted.jsonl
real    1m33.501s
data/candidates_planted.jsonl  102.6 MB  pairs=33718
```

98 of the 100 planted pairs came back as candidates at `floor=0.3`. Both misses are the
`combined` mutation (rename plus literals plus insert applied together); `rename`,
`insert` and `literals` recovered 25 of 25 each. The function count moved from 4327 to
4354 between runs because the source repos are live and drift between runs.

The rest of this section describes the pre-fix behaviour and is kept for the record.

`data/candidates.jsonl` for that second run is 16 GB. The pair count is dominated by
LSH recall, not by structural matches: the 14665 functions form 12642 structural hash
groups (822 with more than one member), which contribute only 21030 pairs. The LSH
neighbourhood at `floor=0.3` has a median of 882 and a mean of 1327 members per
function, which accounts for the remaining ~9.7M pairs. `candidate_pairs` passes
`floor` to `LshIndex` but does not filter emitted pairs by the resulting jaccard, and
MinHashLSH returns matches well under its nominal threshold, so a tail sample of the
output is 100% below 0.3, down to jaccard 0.023.
