# Fingerprinting spike (throwaway)

Measures whether structural hash + MinHash + signature vector finds near-duplicate
TypeScript functions at >= 85% precision at 90% recall on the founder's repos.

Run: `.venv/Scripts/python -m pytest` for tests. Pipeline commands are in
`docs/superpowers/plans/2026-09-05-fingerprint-spike.md`. The result lives in `REPORT.md`.

The headline in `REPORT.md` reproduces from the local evidence in `labels-2026-09-05/`
(`labels.jsonl` and `sample_keys.json`) plus the same-snapshot `data/candidates.jsonl`, `data/candidates_planted.jsonl` and `data/planted.jsonl`, by
re-running `fp.evaluate` with `--status` and `--extra report-extra.md`; that snapshot cannot
be regenerated, because the source repos drift and every rebuild draws a different corpus.
The label set in `labels-2026-09-05/` and the spot-check in `SPOTCHECK-50.md` are kept out
of this repository and held only on the founder's machine, because they quote private source.

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
functions=4373 planted=100 -> data/candidates_planted.jsonl, data/planted.jsonl
real    1m44.681s
data/candidates_planted.jsonl  105.4 MB  pairs=34046
```

99 of the 100 planted pairs came back as candidates at `floor=0.3`: `rename` 25 of 25,
`literals` 25 of 25, `combined` 25 of 25, `insert` 24 of 25. The function count drifts
between runs (4327, then 4354, now 4373) because the source repos are live.

An earlier version of this run reported 98 of 100, but that number was inflated. A
mutation can silently no-op: 42.5% of the corpus has no quoted string (so `literals`
returns the input unchanged) and 6.9% has no line ending in `{` (so `insert` does).
`plant` counted those byte-identical copies as planted duplicates, which the matcher
then found for free. Replaying the old selection shows 16 of its 100 plants were
identical copies, 15 `literals` and 1 `insert`. `plant` now walks a seeded shuffle of
the corpus and skips any base whose mutation does not change the source, so every
planted pair is a genuine near-duplicate. Reaching 100 real plants took 19 skips.

Task 10 regeneration (2026-09-05), both files back to back from one corpus snapshot, then never
regenerated again:

```
functions=4373 pairs=32897 -> data/candidates.jsonl
real    0m19.359s
functions=4373 planted=100 -> data/candidates_planted.jsonl, data/planted.jsonl
real    0m19.181s
```

Labelled 240 pairs (`sample_for_labelling`, seed 7): 89 dup, 145 not, 6 unsure. Result:
precision 0.69 at t=0.70 (recall 0.91), verdict FAIL, tuning pass not run because the condition
did not hold; see `REPORT.md`. The 50-pair blind spot-check for the founder is `SPOTCHECK-50.md`.

The rest of this section describes the pre-LSH-fix behaviour and is kept for the record.

`data/candidates.jsonl` for that second run is 16 GB. The pair count is dominated by
LSH recall, not by structural matches: the 14665 functions form 12642 structural hash
groups (822 with more than one member), which contribute only 21030 pairs. The LSH
neighbourhood at `floor=0.3` has a median of 882 and a mean of 1327 members per
function, which accounts for the remaining ~9.7M pairs. `candidate_pairs` passes
`floor` to `LshIndex` but does not filter emitted pairs by the resulting jaccard, and
MinHashLSH returns matches well under its nominal threshold, so a tail sample of the
output is 100% below 0.3, down to jaccard 0.023.
