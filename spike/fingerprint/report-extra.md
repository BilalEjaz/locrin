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
