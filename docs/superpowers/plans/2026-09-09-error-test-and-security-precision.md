# Locrin engine: error and test rules (plan 3 part A) precision check

Spec 10.2 gate for `swallowed-error`, `test-no-assert` and `test-newly-skipped`,
measured on the five corpus repositories named in the plan's Global Constraints.

Binary: `target/release/locrin.exe`, built from `engine/erosion` at `9edaf4f`
(the three folded fixes of Task 7, before the default change this report causes).
Command per repository: `locrin --root <repo> check --offline`, terminal reporter,
uncapped, with a fresh `LOCRIN_CACHE_DIR` per repository and nothing written into
the repository itself. None of the five has a `locrin.toml`, so this is
out-of-the-box behaviour.

| Repository | Indexed files | Verdict | Findings (all rules) | Wall |
| --- | --- | --- | --- | --- |
| fasting-app | 1846 | BLOCK | 831 (6 high, 167 medium, 658 low) | 12236 ms |
| strongspan | 358 | BLOCK | 179 (1 high, 23 medium, 155 low) | 9004 ms |
| teyji | 207 | BLOCK | 38 (5 high, 4 medium, 29 low) | 4649 ms |
| autoqa | 258 | BLOCK | 57 (17 high, 3 medium, 37 low) | 5603 ms |
| fastlift-admin | 5 | BLOCK | 11 (0 high, 4 medium, 7 low) | 299 ms |

Findings from the three rules under measurement, per repository:

| Rule | fasting-app | strongspan | teyji | autoqa | fastlift-admin | Total |
| --- | --- | --- | --- | --- | --- | --- |
| swallowed-error | 68 | 22 | 4 | 3 | 3 | 100 |
| test-no-assert | 39 | 15 | 0 | 0 | 0 | 54 |
| test-newly-skipped | 1 | 0 | 0 | 0 | 0 | 1 |

## The verdict standard

The same one the plan 2 report used: a finding is true when a maintainer reading
it would change the code, and false when the correct answer is "no, this is
intentional and already decided". That standard matters more here than it did for
the graph rules, because `swallowed-error` is literally right about every finding
it made on this corpus. See the gate section for what that means and for the
reading a founder could take instead.

## Result at a glance

| Rule | Findings | Sampled | True positives | Gate (17/20) |
| --- | --- | --- | --- | --- |
| swallowed-error | 100 | 20 | 0/20 | FAIL |
| test-no-assert | 54 | 20 | 0/20 | FAIL |
| test-newly-skipped | 1 | 1 | 1/1 | unmeasured, sample too small |

## swallowed-error: 0/20 true positives (100 total)

The first 20 findings in verdict order, all from fasting-app (the rule produced 68
there, so the sample never reached the next repository). Each was judged by reading
the catch, its body and the function around it.

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| fasting-app | app/_layout.tsx | 216 | false | Comment-only catch: "import retried next launch; never block startup". A v1 upgrade import that is retried on the next launch by design |
| fasting-app | app/food/photo.tsx | 110 | false | Comment-only: "Swallowed on purpose (see above)". Deleting a temp file that may already be gone |
| fasting-app | app/group/[groupId]/index.tsx | 235 | false | Comment-only: "Reporting is best-effort; a failure never disrupts the group space" |
| fasting-app | app/group/[groupId]/index.tsx | 657 | false | Comment-only: a declined health-permission request leaves the connect state, and the pill can be tapped again |
| fasting-app | app/group/mile-board.tsx | 100 | false | Comment-only: "best-effort: a failed submit never disturbs the board read" |
| fasting-app | app/group/public.tsx | 169 | false | Comment-only: the same declined-permission catch as index.tsx L657 |
| fasting-app | app/group/public.tsx | 386 | false | Comment-only: "Reporting is best-effort; a failure never disrupts the public room" |
| fasting-app | scripts/reset-project.js | 96 | false | Form 2, and wrong twice: the catch does log the error with its message, and the "caller uses the result" it found is `moveDirectories(userInput).finally(() => rl.close())`, a promise chained for cleanup rather than a result read |
| fasting-app | src/db/repositories/foodEntries.ts | 94 | false | Comment-only: "Swallowed deliberately (see above); the primary write has already landed" |
| fasting-app | src/features/account/deleteAccount.ts | 62 | false | Comment-only, in a function called `reconnectQuietly` whose doc comment exists to say the failure must never mask the delete result |
| fasting-app | src/features/account/deleteAccount.ts | 88 | false | Comment-only: the encrypted store not being ready is the only realistic failure and leaks nothing |
| fasting-app | src/features/account/exportData.ts | 64 | false | Comment-only: "user dismissed / share unavailable: not an export failure"; the function returns `{ ok: true }` after it |
| fasting-app | src/features/circuit/cueAudio.ts | 47 | false | Comment-only: "one unloadable asset silences ONE cue, never the feature" |
| fasting-app | src/features/circuit/cueAudio.ts | 72 | false | Comment-only: "a cue never blocks the session" |
| fasting-app | src/features/circuit/cueAudio.ts | 82 | false | Comment-only: "releasing a dead player is not an error worth surfacing" |
| fasting-app | src/features/circuit/cues.ts | 155 | false | Comment-only: "a haptic can never break a session" |
| fasting-app | src/features/circuit/cues.ts | 200 | false | Comment-only: "a cue never blocks the session" |
| fasting-app | src/features/food/FoodDayDial.tsx | 70 | false | Comment-only: haptics unavailable on this device, and the surrounding comment records the device fix that made the try/catch necessary |
| fasting-app | src/features/food/WeekDial.tsx | 67 | false | Comment-only: the same haptic guard, cross-referenced to the FoodDayDial fix |
| fasting-app | src/features/food/useDaySwipe.ts | 95 | false | Comment-only: the same haptic guard, with the step deliberately taken before it |

### The dominant false-positive pattern

One pattern, and it is the whole sample: **the commented deliberate catch**. Across
all 100 findings on all five repositories the split is

| Form | Count |
| --- | --- |
| Empty catch whose body is a comment | 89 |
| Empty catch with a bare body | 0 |
| Log-only catch (form 2) | 6 |
| Floating promise (form 3) | 5 |

Not one bare empty catch exists anywhere in 2674 indexed files. Every empty catch
the corpus contains already carries a comment saying the swallow is deliberate and
naming what happens instead. The rule's module doc anticipates this and answers it
with `locrin:allow`, which is a defensible design; what the corpus adds is the
scale. Turning the rule on out of the box asks a mature repository to annotate or
baseline about ninety decisions it has already made and written down, at Medium
severity, which blocks.

The two smaller forms are worth recording separately because they are not the same
story. All 5 floating-promise findings are in strongspan and are the same shape: a
`useFocusEffect` that defines a local `async function load()` with its own internal
try/catch and then calls it as a statement before returning a cleanup. The promise
is genuinely floating, and it also genuinely cannot reject. Of the 6 log-only
findings, the one in the sample is a false positive for the reason in its row, and
`fasting-app/src/services/widgetPublish.ts` L75 and `autoqa/apps/runner/src/audit-worker.ts`
L26 are the same shape as it: the catch logs, the function returns nothing useful,
and the "result" the rule saw was a promise being chained. Form 2's rule for "a
caller uses the result" counts `f().finally(...)`, `f().then(...)` and `f().catch(...)`
as uses, and on this corpus that is what it mostly finds.

## test-no-assert: 0/20 true positives (54 total)

The first 20 findings in verdict order, all from fasting-app. Each case was read in
full.

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 96 | false | `render(<GroupHubRoute />).getByText('REDIRECT:/')`; the query throws when the redirect did not render, so it is the assertion |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 100 | false | Same form, GroupCreateRoute |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 118 | false | Same form, GroupConsentRoute |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 122 | false | Same form, GroupNameRoute |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 126 | false | Same form, GroupSpaceRoute |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 131 | false | Same form, GroupRenameRoute |
| fasting-app | app/group/flagOff.route.guard.test.tsx | 136 | false | Same form, ChallengeNewRoute |
| fasting-app | app/group/mileBoardGate.guard.test.tsx | 145 | false | `await findByTestId('mile-board-board')` then two `getByText` calls; `findBy` rejects on timeout |
| fasting-app | app/group/mileBoardGate.guard.test.tsx | 204 | false | `await findByTestId` then `getByTestId('mile-board-readiness')`, which is the whole point of the case |
| fasting-app | app/group/roundsChallenge.guard.test.tsx | 173 | false | `await waitFor(() => screen.getByText(C.roundsCaption(7)))`; waitFor rejects when the query never succeeds |
| fasting-app | app/group/roundsChallenge.guard.test.tsx | 182 | false | Two `waitFor(getByText(...))` calls, one at each end of the ladder |
| fasting-app | app/onboarding/concierge.route.test.tsx | 135 | false | `await waitFor(() => utils.getByLabelText('Connected. Steps and sleep will show on Today.'))` |
| fasting-app | app/onboarding/concierge.route.test.tsx | 145 | false | Same form, asserting the declined label |
| fasting-app | app/onboarding/concierge.route.test.tsx | 156 | false | Same form, asserting the unavailable label |
| fasting-app | app/team/flagOff.route.guard.test.tsx | 54 | false | `render(<TeamJoinRoute />).getByText('REDIRECT:/')` |
| fasting-app | app/team/flagOff.route.guard.test.tsx | 58 | false | Same form, TeamConsentRoute |
| fasting-app | app/team/flagOff.route.guard.test.tsx | 62 | false | Same form, TeamNameRoute |
| fasting-app | app/team/flagOff.route.guard.test.tsx | 66 | false | Same form, TeamRoute |
| fasting-app | app/team/flagOff.route.guard.test.tsx | 70 | false | Same form, TeamPlayerDetailRoute |
| fasting-app | src/features/account/FirstFastBackupCard.test.tsx | 53 | false | `await waitFor(() => getByTestId('first-fast-backup-card'))` then two `getByText` calls on the copy |

### The dominant false-positive pattern

One pattern, twenty out of twenty: **the throwing query as the assertion**. React
Native Testing Library's `getBy*` and `getAllBy*` throw when nothing matches,
`findBy*` returns a promise that rejects, and `waitFor` rejects when its callback
never stops throwing. A case written as `render(<Route />).getByText('REDIRECT:/')`
is fully checked; there is nothing to add to it. The rule's own module doc names
this exact blind spot ("a custom matcher named neither `expect` nor `assert` is
invisible") and chose to under-report rather than guess, which was the right call
for a matcher the engine cannot recognise. The corpus says the dialect is not a
minority case in a React Native repository, it is the house style.

Nothing in the sample was a case that genuinely checks nothing. That is not proof
that none exists in the other 34 findings, but the sample gives no evidence that
the rule finds them.

## test-newly-skipped: 1 finding on the corpus

Labelled on whether the case IS skipped, not on whether the skip is new: a
whole-repository run with a fresh cache has no previous version for anything, so
every skipped test is reported once. That is the documented first-run behaviour and
the baseline is what absorbs it.

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| fasting-app | src/db/repositories/foods.test.ts | 173 | true | `it.todo('DATA-04: real-view foods upsert + exercises seed ...')`. The case does not run, and the comment above it says why (a manual device gate covers it), which is exactly the owned decision the rule asks for |

One finding across 2674 indexed files, and the index confirms it: exactly one file
in the whole corpus has a row in `skipped_tests`. Four of the five repositories skip
no tests at all. The rule cannot be measured at the gate's sample size, in either
direction: one finding can neither reach 17/20 nor fail it. What can be said is that
the single finding is correct and the rule produced no noise on 2674 files.

## Gate

Applying the plan's rule (Global Constraints, "Under 17/20 the rule ships
`enabled_by_default() == false` with the reason in its doc comment and in the
precision report"):

- **`swallowed-error`: 0/20, ships off by default.** Doc comment on
  `enabled_by_default` cites the number and this report.
- **`test-no-assert`: 0/20, ships off by default.** Same.
- **`test-newly-skipped`: unmeasured, ships on.** One corpus finding, true, and no
  false positives on 2674 files. It ships on fixture evidence plus that single
  sample, stated as such, the way `unreachable` and `boundary-violation` shipped
  after plan 2.

What does not change: both rules keep their registry entry, so `all_rules()` still
lists eleven and the SARIF `rules` array still describes all of them; both keep
every fixture and every test. The rule tests that assumed the rule was on now turn
it on through `rule_on("swallowed-error")` and `rule_on("test-no-assert")`, which is
what `dead-file`'s tests already did. A repository wanting either rule writes

```toml
[rules.swallowed-error]
enabled = true
```

and gets the same behaviour this report measured.

### The reading a founder could take instead

`swallowed-error` is the one call in this report that could honestly go the other
way, and the difference is worth stating rather than burying. Under the standard
used here (would a maintainer change the code?) it scores 0/20. Under the standard
"is the rule's claim about the code true?" it scores 19/20 and passes: every one of
those catches does drop its error, the comment beside it makes the decision visible
to a reader and not to anything at runtime, and the rule's answer to a repository
that means it is `locrin:allow` on the line. If the founder wants the rule on by
default on that reading, the change is one method and its doc comment in
`crates/rules/src/swallowed_error.rs`, and the two rule tests go back to
`Config::default()`. `test-no-assert` is not in the same position: those twenty
cases genuinely assert, and the rule is simply blind to how.

## Benchmarks after Part A

`cargo test --release -p locrin-cli -- --ignored --nocapture`, three consecutive
runs on `8014911` (Part A complete, both rules off by default). Bench repository
`<home>/fasting-app`, fresh temporary cache per benchmark.

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Verdict |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 7583 ms | 7582 ms | 7037 ms | FAIL |
| warm single-file check | under 300 ms | 251 ms | 232 ms | 243 ms | PASS |
| warm 30-file check | under 1000 ms | 345 ms | 330 ms | 338 ms | PASS |
| startup (`--help`) | under 50 ms | 29 ms | 29 ms | 30 ms | PASS |

### The cold-index failure

It is not caused by Task 7 and it is not new: `4f91e02`, the branch head before this
task, measures 8497 ms, so the three folded fixes and the two default changes made
the cold path slightly faster rather than slower. Bisected on this machine, today:

| Commit | What it is | Cold scan |
| --- | --- | --- |
| `d195857` | Task 1, before `skipped_tests` | 4996 ms |
| `0aff734` | Task 3, records skipped tests per file | 7470 ms |
| `4f91e02` | Task 6, branch head before Task 7 | 8497 ms |
| `8014911` | Task 7 complete | 7037 to 7583 ms |

Two causes, both real:

1. **Task 3 added about 2 s.** `indexer::record` calls `testcases::extract(file)`
   for every test file, and `record` runs on the single thread that owns the index
   connection, not in the rayon parse pass. Replacing that call with an empty vector
   and re-measuring gives 5478 ms, so the extraction is roughly 2 s of the 7.5 s on
   a repository as heavily tested as fasting-app. The fix is to compute the skipped
   set in the parallel pass beside the parse and hand it to `record`, which changes
   a core signature and is more than a close-out task should do unasked.
2. **The machine is slower than when the target was set.** The same Task 1 commit
   measured 3607 ms when Task 1 ran and 4996 ms today, about 38 percent. Plan 2's
   end-of-plan numbers were 2962 to 3274 ms. Even with Task 3's cost removed
   entirely, today's machine gives 5478 ms against a 5000 ms target.

Plan 2's instruction for this benchmark was "fix the cause; do not raise the
target", and neither cause is fixed here. This is flagged for the founder as the one
open item of Part A: the parallelisation of `testcases::extract` is a contained
piece of work, and the machine's baseline should be re-measured on a quiet box
before anyone concludes the target moved.
