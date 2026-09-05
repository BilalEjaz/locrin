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

`data/candidates.jsonl` for that second run is 16 GB. The pair count is dominated by
LSH recall, not by structural matches: the 14665 functions form 12642 structural hash
groups (822 with more than one member), which contribute only 21030 pairs. The LSH
neighbourhood at `floor=0.3` has a median of 882 and a mean of 1327 members per
function, which accounts for the remaining ~9.7M pairs. `candidate_pairs` passes
`floor` to `LshIndex` but does not filter emitted pairs by the resulting jaccard, and
MinHashLSH returns matches well under its nominal threshold, so a tail sample of the
output is 100% below 0.3, down to jaccard 0.023.
