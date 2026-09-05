# Fingerprinting spike report (2026-09-05)
Status: PROVISIONAL. Labels were assigned by Claude with written reasons; the founder's blind spot-check of 50 pairs (SPOTCHECK-50.md) has not yet been done. Below 90 percent agreement, all 240 pairs are relabelled and this report is regenerated.

## Headline
With signature gate: precision 0.69 at t=0.70 (recall 0.91, 116 labelled pairs predicted at t)
Without signature gate: precision 0.69 at t=0.70 (recall 0.91, 116 labelled pairs predicted at t)
Population-weighted precision at the chosen threshold: 0.61 (sample-pooled 0.69)

**Verdict: FAIL: move already-exists to release two**

## Recall by mutation kind, gate on, t=0.70
| mutation kind | recall |
|---|---|
| rename | 1.00 |
| insert | 0.96 |
| literals | 0.96 |
| combined | 0.72 |

## Definitions
- Recall at t: fraction of 100 planted near-duplicates retrieved at threshold t.
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
functions_in_pairs=3543 candidate_pairs=32897 labelled=240 planted=100
bucket populations (all candidate pairs, not the 240 sampled):
- structural match=806
- jaccard >= 0.60, no structural match=1728
- jaccard 0.30 to 0.60, no structural match=30363

## Sweep, gate on
| t | recall | precision | labelled predicted |
|---|---|---|---|
| 0.30 | 0.99 | 0.40 | 225 |
| 0.35 | 0.98 | 0.44 | 201 |
| 0.40 | 0.98 | 0.49 | 182 |
| 0.45 | 0.97 | 0.52 | 169 |
| 0.50 | 0.97 | 0.55 | 161 |
| 0.55 | 0.97 | 0.56 | 157 |
| 0.60 | 0.96 | 0.57 | 154 |
| 0.65 | 0.95 | 0.63 | 133 |
| 0.70 | 0.91 | 0.69 | 116 |
| 0.75 | 0.89 | 0.73 | 97 |
| 0.80 | 0.81 | 0.74 | 94 |
| 0.85 | 0.77 | 0.77 | 88 |
| 0.90 | 0.66 | 0.80 | 81 |
| 0.95 | 0.59 | 0.81 | 80 |

## Sweep, gate off
| t | recall | precision | labelled predicted |
|---|---|---|---|
| 0.30 | 0.99 | 0.38 | 234 |
| 0.35 | 0.98 | 0.43 | 205 |
| 0.40 | 0.98 | 0.49 | 183 |
| 0.45 | 0.97 | 0.52 | 169 |
| 0.50 | 0.97 | 0.55 | 161 |
| 0.55 | 0.97 | 0.56 | 157 |
| 0.60 | 0.96 | 0.57 | 154 |
| 0.65 | 0.95 | 0.63 | 133 |
| 0.70 | 0.91 | 0.69 | 116 |
| 0.75 | 0.89 | 0.73 | 97 |
| 0.80 | 0.81 | 0.74 | 94 |
| 0.85 | 0.77 | 0.77 | 88 |
| 0.90 | 0.66 | 0.80 | 81 |
| 0.95 | 0.59 | 0.81 | 80 |

## Thresholds to carry into the engine (spec section 3.3)
- SHINGLE_K = 5, NUM_PERM = 128
- jaccard_threshold = 0.70
- signature_gate = on (untested: the gate removed no labelled pair above t=0.40 on this sample; "on" is the tie rule)
- MIN_TOKENS = 40

## Label breakdown by sample bucket
| bucket | dup | not | unsure | precision |
|---|---|---|---|---|
| structural match | 65 | 15 | 0 | 0.81 |
| jaccard >= 0.60, no structural match | 23 | 51 | 6 | 0.31 |
| jaccard 0.30 to 0.60, no structural match | 1 | 79 | 0 | 0.01 |

## Tuning pass
Not run. The rule allows one pass (MIN_TOKENS 40 to 60) only when the verdict is FAIL and precision
climbs sharply just above the chosen t. The verdict is FAIL, but the climb above t=0.70 is gradual:
0.69, 0.73, 0.74, 0.77, 0.80, 0.81 at t=0.70 through 0.95, and it never reaches 0.85. The 80
structural-match pairs in the sample are predicted at every t, and their precision alone is 0.81
(65 dup, 15 not), which dominates the sweep at high t. MIN_TOKENS stays at 40.

## Sample notes
Non-structural pairs at jaccard >= 0.70: 15 dup, 21 not, precision 0.42. 26 of the 89 dup labels
are the same four-line accessibility focus callback repeated across sheets; 15 of the 240 pairs
cross repos and 27 sit in one file.
