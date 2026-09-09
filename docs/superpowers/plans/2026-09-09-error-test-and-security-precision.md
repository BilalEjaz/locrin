# Locrin engine: error and test rules (plan 3 part A) precision check

Spec 10.2 gate for `swallowed-error`, `test-no-assert` and `test-newly-skipped`,
measured on the five corpus repositories named in the plan's Global Constraints.

Binary: `target/release/locrin.exe`, built from `engine/erosion` at `db10d5d`
(Task 7c: the two rule corrections below, on top of Task 7b). Command per
repository: `locrin --root <repo> check --offline`, terminal reporter, uncapped,
with a fresh `LOCRIN_CACHE_DIR` per repository and nothing written into the
repository itself. None of the five has a `locrin.toml`, and all five were
confirmed unchanged afterwards. A second pass with `--sarif` over the same warm
cache produced the machine-readable finding list the labelling below works from;
it reports the same findings the terminal pass did.

Both rules under measurement shipped `enabled_by_default() == false` at
`db10d5d`, so the measurement binary flips those two methods to `true` and is
thrown away afterwards. Nothing else differs from `db10d5d`, and no corpus
repository was given a config file. The counts in the two tables below are
therefore what the engine would say with both rules on; what it says with the
shipped defaults is five findings fewer, all of them `swallowed-error` in
strongspan, which still blocks on its one High finding: no repository's verdict
depends on either rule.

| Repository | Indexed files | Verdict | Findings (all rules) | Wall |
| --- | --- | --- | --- | --- |
| fasting-app | 1846 | BLOCK | 724 (6 high, 99 medium, 619 low) | 9754 ms |
| strongspan | 358 | BLOCK | 147 (1 high, 6 medium, 140 low) | 959 ms |
| teyji | 207 | BLOCK | 34 (5 high, 0 medium, 29 low) | 332 ms |
| autoqa | 258 | BLOCK | 54 (17 high, 0 medium, 37 low) | 342 ms |
| fastlift-admin | 5 | ADVISORY | 8 (0 high, 1 medium, 7 low) | 46 ms |

Before Task 7c the same five gave 727 / 159 / 34 / 56 / 8 findings; before Task
7b, 831 / 179 / 38 / 57 / 11. The wall column is not the performance number and
should not be read as one: a corpus pass reads every file, so it depends on how
much of the repository the operating system already had in its page cache.
fasting-app's 9754 ms is the first pass over that checkout after a long gap, and
the controlled benchmark run minutes later put the same cold scan at 3750 ms.
The benchmark section is the performance number.

Findings from the three rules under measurement, per repository:

| Rule | fasting-app | strongspan | teyji | autoqa | fastlift-admin | Total | Before 7c | Before 7b |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| swallowed-error | 0 | 5 | 0 | 0 | 0 | 5 | 11 | 100 |
| test-no-assert | 0 | 0 | 0 | 0 | 0 | 0 | 11 | 54 |
| test-newly-skipped | 1 | 0 | 0 | 0 | 0 | 1 | 1 | 1 |

## What Task 7c changed

Task 7b's measurement named one contained lever per rule, and Task 7c is those
two changes plus this re-measure:

1. `swallowed-error` form 2 asks whether a caller reads a value back, not
   whether the call sits anywhere but an expression statement. A statement
   `await f()`, a `void f()` and an `f().finally(g)` chain sequence a call
   without reading anything off it; binding it (`const x = await f()`), passing
   it, returning it or using it in an expression is a use. That removed all six
   of the rule's form 2 findings.
2. `test-no-assert` counts a `throw_statement` anywhere in a case body,
   including inside nested blocks, loops and `if`s. A runner reports a thrown
   error as a failed case, so `if (!ok) throw new Error("<why>")` checks exactly
   what an `expect` would. That removed all eleven of its findings.

Both rules' remaining findings are labelled fresh below.

## The verdict standard

The same one the plan 2 report used: a finding is true when a maintainer reading
it would change the code, and false when the correct answer is "no, this is
intentional and already decided".

## Result at a glance

Every finding was labelled, not the first 20: neither rule now produces twenty.

| Rule | Findings | Sampled | True positives | Gate (17/20) | Default |
| --- | --- | --- | --- | --- | --- |
| swallowed-error | 5 | 5 | 0/5 | FAIL | off |
| test-no-assert | 0 | 0 | not measurable | no findings, unmeasured | on |
| test-newly-skipped | 1 | 1 | 1/1 | unmeasured, sample too small | on |

Before Task 7c: swallowed-error 11 findings, 0/11; test-no-assert 11 findings,
0/11; test-newly-skipped 1 finding, 1/1.

Before Task 7b: swallowed-error 100 findings, 0/20; test-no-assert 54 findings,
0/20; test-newly-skipped 1 finding, 1/1.

## swallowed-error: 0/5 true positives (5 total)

Two of the rule's three forms now find nothing anywhere in 2674 indexed files.
Form 1 finds no catch that is empty of everything, which is the fact the first
measurement recorded from the other side (89 comment-only catches, zero bare
ones). Form 2 finds no log-only catch whose function has a caller that reads a
value back; the six it used to report were `await`, `void` and `.finally`
around functions returning `Promise<void>`, and all six are gone. What is left
is form 3, and all five are labelled below.

| Repo | File | Line | Form | Verdict | Reason |
| --- | --- | --- | --- | --- | --- |
| strongspan | src/app/session/[id].tsx | 234 | 3 | false | `load()` in a `useEffect`: the async function's whole body is one try/catch that turns any failure into `setScreenState({ status: 'error' })`, so the promise it returns cannot reject |
| strongspan | src/app/workout/[sessionId].tsx | 436 | 3 | false | The same shape |
| strongspan | src/app/workout/complete.tsx | 305 | 3 | false | The same shape |
| strongspan | src/components/WeekView.tsx | 292 | 3 | false | The same shape, `checkInProgress()` in an effect that writes `setInProgress({ status: 'idle' })` on failure |
| strongspan | src/components/WeekView.tsx | 374 | 3 | false | The same shape |

### The one remaining false-positive pattern

**Form 3: the promise that cannot reject.** All five are one React idiom: an
effect declares `async function load()` whose entire body is a try/catch writing
every failure into component state, calls it as a statement, and returns a
cleanup that flips a `cancelled` flag. The promise is genuinely unheld, and it is
also genuinely incapable of rejecting, so nothing is dropped. The finding asks
for a `void` marker on a call that already handles everything.

This is not the same kind of lever as the two Task 7c took. Both of those were
about reading a call site more carefully, which is what the rule already does.
Reading form 3 correctly means reading the callee's own body and deciding
whether it can reject, so it needs the enclosing function's shape to be part of
the judgement rather than the call alone. That is a bigger change than either
correction here and it is not made.

## test-no-assert: no findings on the corpus

Zero across 2674 indexed files, from 54 two measurements ago and 11 one
measurement ago. Both drops closed a blind spot the rule's own module doc had
named as a cost: a Testing Library query that throws (Task 7b) and a bare
`throw` (Task 7c) are checks, and reading them as nothing was the rule's
mistake, not the corpus's style. Nothing was traded for the second drop that the
corpus can see: none of the flag fixture's assertion-free cases moved, and the
rule still reports a case whose body only calls a helper that asserts two levels
down.

What the corpus cannot say is whether the rule finds anything, because these
five repositories have no case that runs code and checks nothing. The gate reads
that as unmeasured (Global Constraints: "A rule that produces no findings on the
corpus is unmeasured and ships on fixture evidence, stated as such"), and the
fixture evidence is what stands behind it: a case that mounts a component and
asserts nothing, one that renders and asserts nothing, and one whose only query
is a non-throwing `queryBy*`.

The known cost of the `throw` ruling, pinned in
`crates/core/src/testcases.rs`: a case that catches its own call and rethrows
reads as asserting. The rethrow does fail the case, so the reading is not wrong
so much as generous; separating it from a guard needs to know whether the try
body can fail, which is flow analysis.

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
precision report", and "A rule that produces no findings on the corpus is
unmeasured and ships on fixture evidence, stated as such"):

- **`swallowed-error`: 0/5, stays off by default.** Three samples, on three
  different false-positive classes, and nothing it has said on 2674 real files
  was worth acting on. Five is far under the gate's sample size, so the rule
  cannot reach 17/20 in either direction on this corpus, but the gate's question
  is not close. Two of its three forms are now silent on the corpus and the third
  needs a change of a different order from the two made here.
- **`test-no-assert`: no corpus findings, ships on.** Unmeasured by the gate's
  own definition, on fixture evidence, with the additional fact that it produces
  no noise at all on 2674 real files: 54 findings became 11 and then none, and
  both classes it lost were its own blind spots rather than the corpus's style.
  This is the same standing `test-newly-skipped` ships on, and the same one
  `unreachable` and `boundary-violation` shipped on after plan 2.
- **`test-newly-skipped`: unmeasured, ships on.** Unchanged by Task 7c.

What does not change: `swallowed-error` keeps its registry entry, so
`all_rules()` still lists eleven and the SARIF `rules` array still describes all
of them; it keeps every fixture and every test. Its tests turn it on through
`rule_on("swallowed-error")`, which is what `dead-file`'s tests already did.
`test-no-assert`'s tests no longer need that and run on `Config::default()`. A
repository wanting `swallowed-error` writes

```toml
[rules.swallowed-error]
enabled = true
```

and gets the same behaviour this report measured.

### What the two rules are worth now

`test-no-assert` is done being wrong about the two things the corpus caught it
being wrong about, and it ships. `swallowed-error` is much quieter than it was
(100, then 11, then 5) and it has still never found anything: of the three
classes it produced across the three measurements, the comment-only catch was a
decision already written down, the `await`/`void`/`.finally` caller never read a
result back, and the effect-scoped `load()` cannot reject. Each of the first two
was a fix; the third is a limit of reading one call site at a time.

## Benchmarks after Task 7c

`cargo test --release -p locrin-cli -- --ignored --nocapture`, on `db10d5d`.
Bench repository `<home>/fasting-app`, fresh temporary cache per
benchmark, on an idle machine (no cargo, rustc or locrin process running, CPU at
2 percent before the first run).

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Verdict |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 3750 ms | 3790 ms | 3764 ms | PASS |
| warm single-file check | under 300 ms | 148 ms | 215 ms | 215 ms | PASS |
| warm 30-file check | under 1000 ms | 247 ms | 222 ms | 233 ms | PASS |
| startup (`--help`) | under 50 ms | 17 ms | 18 ms | 18 ms | PASS |

Four cold attempts were made and all four are reported: **6602**, 3750, 3790,
3764 ms. The three above are the last three; the discarded one is the first, and
what it measures is worth writing down rather than hiding. Its warm numbers on
the same invocation were 148 / 216 / 18 ms, faster than every warm number Task 7b
recorded, so the machine was not loaded. What the first run pays for is the
operating system's page cache over the 1846-file checkout: the cold benchmark
reads every file, and the first read after a long gap comes off disk. Every run
after it, cold cache and all, lands within 40 ms of 3764 ms.

Before Task 7c, on `4395a45`: cold 5983 / 5254 / 5152 ms (FAIL), warm
single-file 255 / 232 / 240 ms, warm 30-file 347 / 340 / 350 ms, startup 29 / 29
/ 28 ms.

### The cold index gate is green

Task 7b left this gate red by 152 ms at its best run and said the residual was
the machine rather than the engine, on the evidence that the Task 1 commit
`d195857`, which has none of this plan's work in it, measured 4996 ms on Task 7's
day against 3607 ms when Task 1 ran. That reading is now confirmed from the other
side. Task 7c changed no indexing code at all: both of its commits are rule
logic, one in `swallowed_error::result_is_used`, which does not run during a
`scan`, and one in `testcases::asserts`, which does. A scan calls
`testcases::skipped_names` for every test file, that calls `extract`, and
`extract` computes the assertion-helper set and every case's assertion count
before the caller throws all of it away and keeps the skipped names. The
`throw_statement` arm 7c added is one more `kind()` comparison inside a walk
that was already visiting those nodes, so it costs nothing measurable, which is
why the timing did not move on its account. The cold scan nonetheless moved from
5152 to 3764 ms, and every other benchmark moved with it in the same proportion (warm
single-file 240 to 215 ms, warm 30-file 350 to 233 ms, startup 28 to 18 ms).

So the 152 ms Task 7b could not close was the measuring box, exactly as it
argued, and the engine's own cold budget on a quiet machine has about 1200 ms of
headroom against the 5000 ms target. Plan 2's instruction ("fix the cause; do not
raise the target") was followed and the target did not move. What is worth
carrying forward is the method rather than the number: the first cold run after
a long gap measures the page cache, so a cold benchmark is only meaningful from
the second run on.
