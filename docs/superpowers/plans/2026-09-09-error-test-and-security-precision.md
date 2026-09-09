# Locrin engine: error and test rules (plan 3 part A) precision check

Spec 10.2 gate for `swallowed-error`, `test-no-assert` and `test-newly-skipped`,
measured on the five corpus repositories named in the plan's Global Constraints.

Binary: `target/release/locrin.exe`, built from `engine/erosion` at `4395a45`
(Task 7b: the two rule corrections below, on top of Task 7). Command per
repository: `locrin --root <repo> check --offline`, terminal reporter, uncapped,
with a fresh `LOCRIN_CACHE_DIR` per repository and nothing written into the
repository itself. None of the five has a `locrin.toml`. A second pass with
`--sarif` over the same warm cache produced the machine-readable finding list the
labelling below works from; it reports the same findings the terminal pass did.

Both rules under measurement ship `enabled_by_default() == false` since Task 7,
so the measurement binary flips those two methods to `true` and is thrown away
afterwards. Nothing else differs from `4395a45`, and no corpus repository was
given a config file.

| Repository | Indexed files | Verdict | Findings (all rules) | Wall |
| --- | --- | --- | --- | --- |
| fasting-app | 1846 | BLOCK | 727 (6 high, 102 medium, 619 low) | 5811 ms |
| strongspan | 358 | BLOCK | 159 (1 high, 7 medium, 151 low) | 1292 ms |
| teyji | 207 | BLOCK | 34 (5 high, 0 medium, 29 low) | 433 ms |
| autoqa | 258 | BLOCK | 56 (17 high, 2 medium, 37 low) | 529 ms |
| fastlift-admin | 5 | ADVISORY | 8 (0 high, 1 medium, 7 low) | 100 ms |

Before Task 7b the same five repositories gave 831 / 179 / 38 / 57 / 11 findings
and 12236 / 9004 / 4649 / 5603 / 299 ms, and fastlift-admin blocked rather than
advising: its four Medium findings were three comment-only catches and one
commented-out block, and losing the catches leaves nothing above Low. The wall
times moved for two reasons and only one of them is the engine (see the benchmark
section); a corpus pass reads every file, so its wall depends on how much of the
repository the operating system already had in its page cache, which is why the
controlled cold-index benchmark rather than this column is the performance
number.

Findings from the three rules under measurement, per repository:

| Rule | fasting-app | strongspan | teyji | autoqa | fastlift-admin | Total | Before 7b |
| --- | --- | --- | --- | --- | --- | --- | --- |
| swallowed-error | 3 | 6 | 0 | 2 | 0 | 11 | 100 |
| test-no-assert | 0 | 11 | 0 | 0 | 0 | 11 | 54 |
| test-newly-skipped | 1 | 0 | 0 | 0 | 0 | 1 | 1 |

## What Task 7b changed

Task 7's measurement produced two corrections to the rules themselves, both
driven by the false-positive patterns it found, and this report is the re-measure
after them:

1. `swallowed-error` form 1 fires only on a catch body with nothing in it at all.
   A body holding only a comment is a maintainer writing the decision down, which
   is what the rule asks for. That removed 89 of its 100 findings.
2. `test-no-assert` counts a Testing Library throwing query (`getBy*`,
   `getAllBy*`, `findBy*`, `findAllBy*`, bare or through `screen` / `within`) as
   an assertion, because those queries throw when they find nothing. `queryBy*`
   still counts as nothing. That removed 43 of its 54 findings.

Neither rule's remaining findings are the ones the first sample judged; both
sections below are labelled afresh.

## The verdict standard

The same one the plan 2 report used: a finding is true when a maintainer reading
it would change the code, and false when the correct answer is "no, this is
intentional and already decided".

## Result at a glance

Every finding was labelled: after Task 7b neither rule produces twenty, so the
sample is the whole population rather than its first 20.

| Rule | Findings | Sampled | True positives | Gate (17/20) |
| --- | --- | --- | --- | --- |
| swallowed-error | 11 | 11 | 0/11 | FAIL |
| test-no-assert | 11 | 11 | 0/11 | FAIL |
| test-newly-skipped | 1 | 1 | 1/1 | unmeasured, sample too small |

Before Task 7b: swallowed-error 100 findings, 0/20; test-no-assert 54 findings,
0/20; test-newly-skipped unchanged. Both rules lost their dominant false-positive
class and neither found a true positive behind it.

## swallowed-error: 0/11 true positives (11 total)

Form 1 produced nothing at all. With the comment-only catch exempt, 2674 indexed
files contain no catch that is empty of everything, which is the same fact the
first measurement recorded from the other side (89 comment-only catches, zero
bare ones). What is left is six log-only catches (form 2) and five floating
promises (form 3), and all eleven are labelled below.

| Repo | File | Line | Form | Verdict | Reason |
| --- | --- | --- | --- | --- | --- |
| fasting-app | scripts/reset-project.js | 96 | 2 | false | Wrong twice, as in the first measurement: the catch does log the error with its message, and the "caller that uses the result" is `moveDirectories(userInput).finally(() => rl.close())`, a promise chained for cleanup |
| fasting-app | src/services/sync.ts | 559 | 2 | false | `bridgeWeighInsOnConnect` returns `Promise<void>` and its doc comment says it never throws and retries on the next connect event. The only same-file call is `void bridgeWeighInsOnConnect()`, which is a statement saying the result is deliberately dropped |
| fasting-app | src/services/widgetPublish.ts | 75 | 2 | false | `publishWidgetData` returns `Promise<void>`; the only same-file call is `await publishWidgetData(now)` inside `refreshWidgets`, which sequences it rather than reading anything back. The catch is a PII-free breadcrumb and the widget keeps its last payload by design |
| strongspan | scripts/reset-project.js | 98 | 2 | false | The same Expo template script as the fasting-app row, same reason |
| strongspan | src/app/session/[id].tsx | 234 | 3 | false | `load()` in a `useEffect`: the async function's whole body is one try/catch that turns any failure into `setScreenState({ status: 'error' })`, so the promise it returns cannot reject |
| strongspan | src/app/workout/[sessionId].tsx | 436 | 3 | false | The same shape |
| strongspan | src/app/workout/complete.tsx | 305 | 3 | false | The same shape |
| strongspan | src/components/WeekView.tsx | 292 | 3 | false | The same shape, `checkInProgress()` in a `useFocusEffect` |
| strongspan | src/components/WeekView.tsx | 374 | 3 | false | The same shape |
| autoqa | apps/runner/src/audit-worker.ts | 26 | 2 | false | `generateAuditReport` returns `Promise<void>` and its doc says a missing API key or an LLM error is logged and never fatal because the audit itself already succeeded. The caller is `await generateAuditReport(...)` |
| autoqa | apps/runner/src/scan-worker.ts | 26 | 2 | false | The same function one file over, same reason |

### The two remaining false-positive patterns

**Form 2: `await` and `void` are not uses.** `result_used_elsewhere` counts a
call whose parent is not an expression statement as a caller using the result.
Every spelling that appears in this corpus around a function returning
`Promise<void>` has such a parent: `await f()` is an await expression, `void f()`
is a unary expression, `f().finally(g)` puts the call inside a member expression.
So a void async function whose catch logs is reported as soon as the file calls
it at all, which is all six findings. What would separate them is the enclosing
function's own shape rather than its call sites: a function with no `return
<expr>` anywhere in its body has no result for a caller to use, and every one of
these six is that. Not changed here; it is a fourth rule change and Task 7b was
scoped to three.

**Form 3: the promise that cannot reject.** All five are the same React idiom: an
effect declares `async function load()` whose entire body is one try/catch
writing every failure into component state, calls it as a statement, and returns
a cleanup that flips a `cancelled` flag. The promise is genuinely unheld, and it
is also genuinely incapable of rejecting, so nothing is dropped. The finding asks
for a `void` marker on a call that already handles everything. Reading that needs
to know the callee's body handles its own failures, which is inside one file and
so is reachable, but it is again more than this task.

## test-no-assert: 0/11 true positives (11 total)

The 39 fasting-app findings are gone: every one of them was a throwing-query
case. All eleven that remain are in strongspan, and all eleven are one shape.

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| strongspan | src/__tests__/guards/coachSurfaceAlpha.guard.test.ts | 158 | false | `it.each(COACH_SURFACES)` reads each surface file and `throw new Error(...)` with the threat id when a forbidden pattern matches |
| strongspan | src/__tests__/guards/primaryButtonDisabled.guard.test.ts | 105 | false | Same: throws with the offending tag and the design rule when a `PrimaryButton` is passed `disabled` |
| strongspan | src/domain/catalog/__tests__/exercises.test.ts | 73 | false | Walks `EXERCISES` and throws naming the exercise when a bodyweight increment class carries dumbbell-only equipment |
| strongspan | src/domain/catalog/__tests__/exercises.test.ts | 109 | false | Throws when a substitution group has no flag-free alternative |
| strongspan | src/domain/catalog/__tests__/exercises.test.ts | 136 | false | Throws when a home-track exercise uses equipment outside the allowed three |
| strongspan | src/domain/catalog/__tests__/frozen-ids.test.ts | 104 | false | Throws when a frozen exercise id has left the live catalog, with the reason ids may never be renamed |
| strongspan | src/domain/programs/__tests__/templates.test.ts | 71 | false | Throws naming the slot when a gym `exerciseId` does not resolve |
| strongspan | src/domain/programs/__tests__/templates.test.ts | 83 | false | The same for home ids |
| strongspan | src/domain/programs/__tests__/templates.test.ts | 95 | false | Throws when a home id's catalog entry is not in the home track |
| strongspan | src/domain/programs/__tests__/templates.test.ts | 160 | false | Throws when an exercise id appears in both Day A and Day B |
| strongspan | src/domain/programs/__tests__/templates.test.ts | 188 | false | Throws when a three-set slot maps to a non-compound catalog entry |

### The dominant false-positive pattern

**The explicit `throw` as the assertion.** Every one of the eleven walks a data
set in a loop and throws a written explanation when an invariant breaks. A
runner reports a thrown error as a failed case, so these check exactly as much as
an `expect` would; what they do not do is name a matcher, and per-item loops are
where the style pays off because the message can name the item.

The lever is small and known: count a `throw_statement` inside a case body as an
assertion, in the same `count_assertions` walk that Task 7b added the throwing
queries to. It is not done here for the reason given under form 2 above. Until it
is, this rule has no measurable true-positive rate: two independent samples, on
two different false-positive classes, and still nothing it found was worth
acting on.

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

- **`swallowed-error`: 0/11, stays off by default.** Every finding it makes on the
  corpus was labelled and none was worth acting on. Eleven is under the gate's
  sample size, so the rule cannot reach 17/20 in either direction on this corpus,
  but the gate's question is not close: a rule with zero true positives out of its
  entire corpus output does not ship on.
- **`test-no-assert`: 0/11, stays off by default.** Same reading, and the second
  sample in a row with nothing true in it.
- **`test-newly-skipped`: unmeasured, ships on.** Unchanged by Task 7b. One corpus
  finding, true, and no false positives on 2674 files. It ships on fixture
  evidence plus that single sample, stated as such, the way `unreachable` and
  `boundary-violation` shipped after plan 2.

What does not change: both rules keep their registry entry, so `all_rules()` still
lists eleven and the SARIF `rules` array still describes all of them; both keep
every fixture and every test. The rule tests that assumed the rule was on turn it
on through `rule_on("swallowed-error")` and `rule_on("test-no-assert")`, which is
what `dead-file`'s tests already did. A repository wanting either rule writes

```toml
[rules.swallowed-error]
enabled = true
```

and gets the same behaviour this report measured.

### What the two rules are worth now

Both are much quieter and much more nearly right than they were, and neither has
yet found anything. That is the honest summary: 89 of `swallowed-error`'s 100
findings and 43 of `test-no-assert`'s 54 were classes of false positive that are
now gone, and behind them was not a single true positive on 2674 files.

The first report's "reading a founder could take instead" no longer applies to
`swallowed-error`: it argued the rule scored 19/20 under "is the claim true?"
because the comment-only catches really do drop their errors. Those findings are
gone by ruling, and the eleven that remain are not true under either standard:
form 2 misreads `await` and `void` as uses of a result, and form 3 flags promises
that cannot reject.

Each rule now has one contained next lever, both named in its section above (form
2's "no `return <expr>` means no result", and counting a `throw` statement as an
assertion). Either is a small change with a corpus behind it. Neither is done
here, and until one is, the measured position is the one in the table.

## Benchmarks after Task 7b

`cargo test --release -p locrin-cli -- --ignored --nocapture`, three consecutive
runs on `4395a45`. Bench repository `<home>/fasting-app`, fresh temporary
cache per benchmark.

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Verdict |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 5983 ms | 5254 ms | 5152 ms | FAIL |
| warm single-file check | under 300 ms | 255 ms | 245 ms | 240 ms | PASS |
| warm 30-file check | under 1000 ms | 347 ms | 340 ms | 350 ms | PASS |
| startup (`--help`) | under 50 ms | 29 ms | 29 ms | 28 ms | PASS |

Before Task 7b, on `8014911`: cold 7583 / 7582 / 7037 ms (FAIL), warm single-file
251 / 232 / 243 ms, warm 30-file 345 / 330 / 338 ms, startup 29 / 29 / 30 ms.

### The cold index after the fix

Task 7 bisected the cold regression to two causes and Task 7b fixed the one that
was ours. `testcases::extract` no longer runs inside `indexer::record_with_stat`
on the single thread that owns the index connection; the skipped-case names are
computed in the rayon parse pass beside `parse_source` and handed to the recorder.
Measured effect: the worst run improves by 1600 ms and the best by 1885 ms, which
matches Task 7's estimate of about 2 s from the stub experiment (5478 ms).

The benchmark is still red by 152 ms at its best run, and the remaining cause is
the second one Task 7 named, which no code change on this branch can address:

| Commit | What it is | Cold scan, measured today or on Task 7's day |
| --- | --- | --- |
| `d195857` | Task 1, before `skipped_tests` | 3607 ms when Task 1 ran, 4996 ms on Task 7's day |
| `8014911` | Task 7 complete | 7037 to 7583 ms |
| `4395a45` | Task 7b, extraction parallelised | 5152 to 5983 ms |

The Task 1 commit, which has none of this plan's work in it at all, measures
within 4 ms of the target on this machine. So the engine's own budget above that
floor is a handful of milliseconds, and the 152 ms gap is not a second piece of
engine work waiting to be found; it is the target having been set on a faster
machine. Plan 2's instruction stands ("fix the cause; do not raise the target"),
and the cause that was fixable has been fixed. What is owed before anyone moves
the target is a re-measure of the Task 1 baseline on a quiet box, which is a
founder call rather than a code change.
