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

## Part B: the security pack (plan 3 part B)

Spec 10.2 gate for the ten security rules, measured on the same five corpus
repositories, by the same standard, so the two halves of this document can be
read against each other.

### Method

Binary: `target/release/locrin.exe`, built from `engine/security` at `d5d0865`
(Task 16 part 1, the directory-argument lockfile fix). Command per repository:
`locrin --root <repo> check --offline`, terminal reporter, uncapped, with a
fresh `LOCRIN_CACHE_DIR` per repository and nothing written into the repository
itself. A second pass with `--sarif` over the same warm cache produced the
machine-readable finding list the labelling works from. None of the five has a
`locrin.toml`, so every rule ran on its shipped defaults, and every one of the
ten was on by default at `d5d0865`.

`vulnerable-dependency` is the exception to `--offline`. It answers from a
cached advisory snapshot, no corpus repository had one, and offline with no
snapshot the rule skips with a warning, which is what all five printed. It was
therefore measured once online, on fasting-app only, with its own fresh cache.
That is the only network call any measurement in this document made.

Several rules were tightened after the task that first measured them, so every
count below was re-run at `d5d0865` rather than copied forward. The labels are
the ones the task reports argued for; where a label is restated here against
this document's verdict standard rather than the task's, it says so.

One rule changed default after this measurement: `injection-sink` ships off, at
`6e583e9`, for the reason in the Gate section. The counts below are what the
engine says with all ten on. With the shipped defaults it says 28 findings
fewer, all `injection-sink`: fasting-app 724 and strongspan 142, which are
exactly the Part A numbers. No repository's verdict depends on any Part B rule.

| Repository | Indexed files | Verdict | Findings (all rules) | Wall |
| --- | --- | --- | --- | --- |
| fasting-app | 1846 | BLOCK | 727 (9 high, 99 medium, 619 low) | 6182 ms |
| strongspan | 358 | BLOCK | 167 (26 high, 1 medium, 140 low) | 1448 ms |
| teyji | 207 | BLOCK | 36 (7 high, 0 medium, 29 low) | 668 ms |
| autoqa | 258 | BLOCK | 54 (17 high, 0 medium, 37 low) | 649 ms |
| fastlift-admin | 5 | ADVISORY | 8 (0 high, 1 medium, 7 low) | 258 ms |

The wall column is not the performance number, for the reason Part A gives. The
benchmark section is.

Findings from the ten rules under measurement, per repository:

| Rule | fasting-app | strongspan | teyji | autoqa | fastlift-admin | Total |
| --- | --- | --- | --- | --- | --- | --- |
| secret-exposed | 0 | 0 | 0 | 0 | 0 | 0 |
| weak-crypto | 0 | 0 | 1 | 0 | 0 | 1 |
| injection-sink | 3 | 25 | 0 | 0 | 0 | 28 |
| html-injection | 0 | 0 | 1 | 0 | 0 | 1 |
| vulnerable-dependency (offline) | 0 | 0 | 0 | 0 | 0 | 0 |
| vulnerable-dependency (online) | 54 | not run | not run | not run | not run | 54 |
| supabase-service-role-in-client | 0 | 0 | 0 | 0 | 0 | 0 |
| supabase-table-without-rls | 0 | 0 | 0 | 0 | 0 | 0 |
| express-route-without-auth | 0 | 0 | 0 | 0 | 0 | 0 |
| express-cors-wildcard-on-authenticated | 0 | 0 | 0 | 0 | 0 | 0 |
| express-cookie-insecure | 0 | 0 | 0 | 0 | 0 | 0 |

### Result at a glance

The verdict standard is Part A's, unchanged: a finding is true when a maintainer
reading it would change the code, and false when the correct answer is "no, this
is intentional and already decided".

| Rule | Findings | Sampled | True positives | Gate (17/20) | Default |
| --- | --- | --- | --- | --- | --- |
| secret-exposed | 0 | 0 | not measurable | unmeasured, fixture evidence only | on (locked) |
| weak-crypto | 1 | 1 | 0/1 | unmeasured, sample too small | on |
| injection-sink | 28 | 28 (all) | 2/28 | FAIL | off |
| html-injection | 1 | 1 | 1/1 | unmeasured, sample too small | on |
| vulnerable-dependency | 54 | 54 against OSV, 5 by hand | 54/54 | PASS | on |
| supabase-service-role-in-client | 0 | 0 | not measurable | unmeasured, fixture evidence only | on |
| supabase-table-without-rls | 0 | 0 | not measurable | unmeasured, fixture evidence only | on |
| express-route-without-auth | 0 | 0 | not measurable | unmeasured, rule never ran | on |
| express-cors-wildcard-on-authenticated | 0 | 0 | not measurable | unmeasured, no Express in the corpus | on |
| express-cookie-insecure | 0 | 0 | not measurable | unmeasured, no Express in the corpus | on |

Six of the ten produced nothing at all, and one produced nothing offline. The
corpus measures three of them.

### secret-exposed: 0 findings, and the zero was checked

Zero across 2674 indexed files. `secret-exposed` is `locked` (spec 4.3), so a
false positive here is a STOP for the founder and there is none.

A zero from a locked rule is worth a probe rather than a shrug, so it got the
same treatment Task 14 gave the Supabase zeros. A scratch repository outside
every corpus checkout, holding one file with a GitHub token shaped value, is
reported at its real line as `GitHub token credential: ghp_...(40 chars)`: the
provider and a mask, never the secret, which is the Global Constraints rule. The
same file's `AKIAIOSFODNN7EXAMPLE` is correctly silent, because that is AWS's own
documentation key and the value carries `EXAMPLE`, which `LINE_PLACEHOLDER`
rejects. So the zero is 2674 files with no committed credential in them and not
a rule that never ran.

Unmeasured by the gate's definition, and it could not ship off in any case.

### weak-crypto: 1 finding, 0 true

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| teyji | packages/api/src/router.integration.test.ts | 215 | false | A throwaway discount code minted by a helper inside an integration test, built from `Date.now()` and `Math.random()`. The claim is literally accurate and in shipped code it would be a real finding; in a fixture nobody acts on it |

Unchanged from Task 9's second run and from Task 10's re-measure. One finding is
a twentieth of the gate's sample, so the rule can neither reach 17/20 nor fail
it here: unmeasured, and it ships on, the same standing Part A gave
`test-newly-skipped`. Forms 1 and 3 (MD5 and SHA-1, static IV) have nothing to
find on this corpus, which Task 9 checked rather than assumed.

### injection-sink: 2 true of 28

Every one of the 28 was labelled rather than a sample of 20, because 28 is close
enough to 20 that a sample would only hide which cluster it drew from.

| Repo | File | Line(s) | Verdict | Reason |
| --- | --- | --- | --- | --- |
| fasting-app | src/services/v1ImportWiring.ts | 92 | true | `UPDATE "${table}" SET ${assignments} WHERE id = ?`. The values are parameterised, but `table` and the column names in `assignments` come from `Object.keys(row)` on a row of an imported v1 payload, so an attacker-chosen key carrying a double quote breaks out of the quoted identifier. An allow-list of tables and columns is the fix and a maintainer would write one |
| fasting-app | src/services/v1ImportWiring.ts | 97 | true | The same shape on the `INSERT` arm |
| fasting-app | scripts/test-all.js | 32 | false | `spawnSync` with `shell: true`, under a five line comment saying the shell is required for `npm.cmd` on Windows, that argv plus shell raises DEP0190, and that "the script names are the two hardcoded constants above, never user input". Intentional and already written down |
| strongspan | src/db/\_\_tests\_\_/migration-upgrade-path.test.ts | 46 | false | `db.exec(stmt)` where `stmt` is one statement of a committed drizzle migration file split on the statement separator. Applying a migration is what the code is for |
| strongspan | src/app/workout/\_\_tests\_\_/complete-outcomes.test.tsx | 171 | false | jest seeding the mocked SQLite with an `INSERT` interpolating file-level constants |
| strongspan | src/services/\_\_tests\_\_/finish.test.ts | 151 | false | The same shape |
| strongspan | src/services/\_\_tests\_\_/history-stability.test.ts | 149, 158 | false | The same shape |
| strongspan | src/services/\_\_tests\_\_/sessions.test.ts | 157, 411, 416, 421, 425, 449, 453, 459, 463, 480, 484, 513, 518, 535, 540, 559, 574, 579, 583, 588 | false | The same shape, twenty times in one file |

#### The dominant false-positive pattern

**SQL assembled in a test fixture from constants declared in the same file.** 24
of the 26 false findings are one idiom: a jest suite mocks the database module,
reaches through to the SQLite handle, and seeds a row with an `INSERT` that
interpolates `ENROLLMENT_ID`, `DAY_ID`, `LOCAL_DATE` and `NOW_MS`, all `const`
declarations at the top of the same file. There is no attacker, no request and
no reachable value: the interpolated expressions are literals one screen up. The
rule's claim is accurate every time and the security finding it implies is
absent every time.

The two that are not that idiom are the same thing viewed differently: a
migration runner executing a committed `.sql` file, and a build script whose
comment already answers the question. Across all 26 the answer is "yes, on
purpose, and here is why", which is exactly the verdict standard's definition of
false.

Task 10 already met this cluster and made a contained ruling: a SQL-family
finding inside a test file drops to Medium confidence. That was right about how
the finding should read and it does not touch how many there are or what blocks
the run, because confidence is not severity. `injection-sink` is High severity,
so on strongspan the 25 test findings took the repository from 1 High finding to
26 and buried its one real one.

Separating a fixture's SQL from a route handler's needs the origin of the
interpolated expression rather than its shape, which is cross-function taint and
is release two by the plan's own scope statement (spec 4.2). It is not a
contained lever of the kind Tasks 7b, 7c and 10 pulled, so none was pulled here.

### html-injection: 1 finding, 1 true

| Repo | File | Line | Verdict | Reason |
| --- | --- | --- | --- | --- |
| teyji | apps/web/lib/ui-host.tsx | 182 | true | `<style dangerouslySetInnerHTML={{ __html: seam.css }} />`. The string is generated CSS rather than user text, so nothing is exploitable today, but the sink is real and the finding is the kind a reviewer reads once and answers with a `locrin:allow` or a sanitiser at the seam. Task 11's label, kept |

One finding, so unmeasured at the gate's sample size in either direction. It
ships on, and it produced no noise on 2674 files.

### vulnerable-dependency: 54 findings on fasting-app, 54 agree with OSV

Offline with no snapshot the rule skips, which every corpus run confirmed with
`warning: no cached advisory snapshot; vulnerable-dependency skipped` on stderr.
Measured online, once, on fasting-app's `package-lock.json`: 54 findings, 42 at
High severity and 12 at Medium, over 33 distinct advisories, from a lockfile
whose npm tree carries three versions of `@xmldom/xmldom`.

Five were hand-checked against OSV's own record for the advisory
(`https://osv.dev/vulnerability/<id>`, read with `urllib` against the same
document that page renders), read-only:

| Advisory | Package | Installed | OSV severity | OSV fixed | Rule agrees? |
| --- | --- | --- | --- | --- | --- |
| GHSA-2883-xcg3-v3hh | js-yaml | 3.15.1 | HIGH | 3.15.2 for the 3.x range (4.3.2 for 4.x) | affected yes, severity yes, fixed version **no**: says 4.3.2 |
| GHSA-jqff-g426-hqxp | fast-uri | 3.1.5 | HIGH | 3.1.6 for the 3.x range (2.4.5 for 2.x) | affected yes, severity yes, fixed version **no**: says 2.4.5, a downgrade |
| GHSA-6g55-p6wh-862q | postcss | 8.4.49 | HIGH | 8.5.12 | yes on all three |
| GHSA-w3rx-r6r6-pgpr | image-size | 1.2.1 | HIGH | none published | yes on all three, and the fix line says so in words |
| GHSA-67mh-4wv8-2f99 | esbuild | 0.18.20 | MODERATE | 0.25.0 | yes on all three, and MODERATE maps to Medium as the plan says |

The same check was then run over all 54 findings against OSV:

- **54 of 54**: the installed version falls inside an OSV affected range for that
  advisory and that npm package. There is no false positive in the set.
- **0 of 54**: severity disagreements. Every rating the rule assigned is the one
  OSV publishes, and the CRITICAL/HIGH to High, MODERATE to Medium mapping holds
  throughout.
- **16 of 54**: the fixed version named in the fix line comes from a different
  range of the advisory than the range the installed version sits in. Five of
  those are outright downgrades. See below.

**54/54 true, PASS.** The rule's claim is "this installed version is affected by
this advisory", and it is right every time.

#### The fix line names the wrong fixed version on 16 of 54

An advisory lists one affected range per maintained branch, each with its own
`fixed` event, and the rule takes a fixed version without asking which range the
installed version is in. The consequences, all reproduced:

| Package | Installed | Rule says upgrade to | OSV's fix for that range |
| --- | --- | --- | --- |
| fast-uri | 3.1.5 | 2.4.5 | 3.1.6 |
| @xmldom/xmldom | 0.9.10 | 0.8.15 | 0.9.12 |
| @xmldom/xmldom | 0.8.13 | 0.9.11 | 0.8.14 |
| @xmldom/xmldom | 0.7.13 | 0.9.11 | 0.8.14 |
| js-yaml | 3.15.1 | 4.3.2 | 3.15.2 |

Two of these tell a reader to install an older release than the one they have,
which would not fix the advisory and would not build. The finding is still true
and the severity is still right, so this does not move the gate, but it is a
wrong instruction inside a correct security finding and it is the loudest thing
this measurement found. It is a Task 13 defect and it is not fixed here, because
Task 16 changes rule logic only where the gate forces a default. It is carried
as concern 1 of the Task 16 report.

#### Fixed, and re-measured online (`d654658`)

`osv::first_fixed` now chooses the range by the installed version rather than by
its position in the document: every SEMVER range is read as the half-open
interval OSV means by it, `introduced <= v < fixed`, and the range holding the
installed version names the upgrade. A range that holds the version and names no
fix, closed by a `last_affected` or opened by an `introduced` with nothing after
it, answers no fix rather than borrowing another branch's, and so does a version
no range holds.

Re-run online against fasting-app with a fresh cache, once. The finding set is
unchanged at **54 findings over the same advisories**; only the fix lines moved:

| Package | Installed | Before | After | OSV's fix for that range |
| --- | --- | --- | --- | --- |
| fast-uri | 3.1.5 | 2.4.5 | **3.1.6** | 3.1.6 |
| @xmldom/xmldom | 0.9.10 | 0.8.15 | **0.9.12** | 0.9.12 |
| @xmldom/xmldom | 0.8.13 | 0.9.11 | **0.8.14** | 0.8.14 |
| @xmldom/xmldom | 0.7.13 | 0.9.11 | **0.8.14** | 0.8.14 |
| js-yaml | 3.15.1 | 4.3.2 | **3.15.2** | 3.15.2 |

Across all 54: **52 name a later version than the installed one and 0 name an
earlier or equal one.** The remaining 2 name no fix at all, both `image-size
1.2.1`, whose advisories close with `{"introduced": "0", "last_affected":
"2.0.2"}` and publish no fixed release; the advisory says so in words, which is
the same answer the hand-check above recorded as correct.

#### Warm cost

Task 13 measured the rule directly on fasting-app with a warm cache: **14.2,
14.4, 15.6 and 16.7 ms** across four runs, against the plan's 50 ms target.
PASS. That is a 1.6 MB `package-lock.json` read and parsed, one SQLite read of
the batch snapshot, nine reads of cached advisory documents and the JSON parsing
of all of it. Since the Task 13 review fix a scoped run does not pay it at all
unless the scope names the lockfile, and since Task 16 part 1 a directory
argument that contains the lockfile counts as naming it.

### The two Supabase rules: 0 findings, both zeros checked

Unmeasured, shipping on fixture evidence. Task 14 probed both zeros rather than
assuming them, and this measurement reproduces its counts:

- `supabase-service-role-in-client`: fasting-app holds 17 files naming
  `service_role` and every one is under `supabase/functions/**`, exempt by the
  first default `server_paths` glob. The zero is 17 correct exemptions. Task 14
  copied one of those files to a client path in a scratch repository and the
  rule reported it at its real line.
- `supabase-table-without-rls`: fasting-app's 35 migrations create 33 tables and
  enable row-level security on all 33. The zero is 33 correct answers. Task 14
  deleted one `enable` line in a scratch copy and the rule named that table, at
  its real `create` line, and said nothing about the other 32.

Neither probe wrote into a corpus checkout.

### The three Express rules: 0 findings, and the corpus cannot measure them

Unmeasured, shipping on fixture evidence, and for a blunter reason than the
Supabase pair: **there is no Express application anywhere in the corpus.** A
`git grep` for `from "express"` and `require("express")` across all five
checkouts returns nothing, and so does one for `res.cookie(` and `cors(`. The
five repositories are two React Native apps, a Next.js and tRPC monorepo, a Node
service on Railway and a Cloudflare Worker.

`express-route-without-auth` additionally never ran at all: by Global
Constraints it runs only when `framework.auth_middleware` is non-empty, that
list is empty by default, and none of the five has a `locrin.toml`. Its zero is
therefore not even an answer.

These three ship on their fixtures with no corpus evidence in either direction,
which is a weaker standing than any other rule in this document has, and the
next repository with an Express server in it is the measurement they still owe.

### Gate

Applying the plan's rule (Global Constraints: "Under 17/20 the rule ships
`enabled_by_default() == false` with the reason in its doc comment and in the
precision report", and "A rule that produces no findings on the corpus is
unmeasured and ships on fixture evidence, stated as such"):

- **`injection-sink`: 2/28, ships off by default.** The only Part B rule the
  corpus measures at anything near the gate's sample size, and it is not close:
  any 20 of the 28 lands at 2/20 or below. It keeps its registry entry, its
  fixtures and every test, so `all_rules()` still lists 21 and the SARIF `rules`
  array still describes all 21; its tests turn it on through
  `rule_on("injection-sink")`, which is what `dead-file` and `swallowed-error`
  already do. A repository that wants it writes

  ```toml
  [rules.injection-sink]
  enabled = true
  ```

  and gets exactly the behaviour measured above.
- **`vulnerable-dependency`: 54/54, ships on.** The only Part B rule that passes
  the gate on measured evidence rather than on an absence, and it passes it
  against an independent source. The fixed-version defect above is a separate
  bug against a rule whose findings are right.
- **`secret-exposed`: 0 findings, ships on, locked.** No miss, so no STOP. The
  zero was probed.
- **`weak-crypto`: 1 finding, 0 true, ships on.** A sample of one cannot fail a
  17/20 gate, and Task 9 recorded it that way at the time. Unmeasured.
- **`html-injection`: 1 finding, 1 true, ships on.** Unmeasured, same reason.
- **Both Supabase rules: 0 findings, ship on.** Unmeasured, with both zeros
  probed as correct answers rather than silence.
- **All three Express rules: 0 findings, ship on.** Unmeasured, and the corpus
  contains no Express application, so this is an absence of evidence and not
  evidence of precision. Stated here so that nobody later reads the zero as a
  pass.

What Part B ships, then, is one rule measured and passing, one measured and
turned off, and eight standing on their fixtures. That is a thinner evidence
base than Part A's, and the reason is the corpus rather than the rules: five
repositories by one founder, none of them an Express server, none with a
committed credential, all of them with row-level security already on.

### Benchmarks after Part B

`cargo test --release -p locrin-cli -- --ignored --nocapture`, on `d5d0865`.
Bench repository `<home>/fasting-app`, fresh temporary cache per
benchmark, on an idle machine (no cargo, rustc or locrin process running before
the first run).

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Verdict |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 6190 ms | 6088 ms | 6209 ms | **FAIL** |
| warm single-file check | under 300 ms | 331 ms | 319 ms | 333 ms | **FAIL** |
| warm 30-file check | under 1000 ms | 504 ms | 442 ms | 448 ms | PASS |
| startup (`--help`) | under 50 ms | 29 ms | 26 ms | 32 ms | PASS |

Four cold attempts, all four reported: **6610**, 6190, 6088, 6209 ms. The first
is discarded by the method Part A established (the first cold run after a build
pays the page cache) and the other three sit inside 121 ms of each other.

**Startup is 26 to 32 ms with `ureq` linked**, against the 50 ms target, so the
one target this plan put at risk by adding a network client is not the one that
went red. Neither target was lowered.

#### Two gates are red, and they are red for two different reasons

The honest way to separate the engine from the machine is to measure the branch
base on the same machine in the same session, so the benchmark was also run at
`8fe38e0`, the merge base of `engine/security`, in a detached worktree:

| Benchmark | Target | `8fe38e0` (base) | `d5d0865` (HEAD) | Part B costs |
| --- | --- | --- | --- | --- |
| cold index | 5000 ms | 5459 / 5120 / 5062 | 6610 / 6190 / 6088 / 6209 | about 1000 ms |
| warm single-file | 300 ms | 226 / 223 / 227 | 362 / 331 / 319 / 333 | about 100 ms |
| warm 30-file | 1000 ms | 319 / 326 / 318 | 453 / 504 / 442 / 448 | about 130 ms |
| startup | 50 ms | 31 / 29 / 29 | 30 / 29 / 26 / 32 | nothing |

So:

1. **The cold gate is red on the base as well.** `8fe38e0` contains none of Part
   B and it measures 5062 to 5459 ms today against the 3750 to 3790 ms Part A
   recorded for the same code on its own day. That is the measuring-box drift
   this ledger has now recorded four times (Task 1's commit at 3607 ms one
   morning and 4996 ms the same afternoon; Task 13 saw 4669 to 5158 ms). About
   1300 ms of the red is the machine.
2. **The warm single-file gate is red and the machine has nothing to do with
   it.** The base passes it today with 73 ms of headroom and HEAD misses it by
   19 to 62 ms. This one is Part B's.

#### What Part B costs, isolated

Controlled measurements, all on this machine, all with fresh caches:

| Root | `8fe38e0` | `d5d0865` |
| --- | --- | --- |
| an empty directory | 68 / 69 / 61 / 64 ms | 70 / 65 / 65 / 60 ms |
| a directory holding one 1-line `.ts` file | 76 / 61 / 51 / 63 ms | 174 / 173 / 148 / 142 ms |
| fastlift-admin (5 files) | 94 / 101 / 119 ms | 220 / 201 / 217 ms |
| teyji (207 files) | 422 / 406 / 426 ms | 611 / 620 / 590 ms |

An empty repository costs exactly what it did. **The first source file costs
about 90 ms more, and every file after it about 0.35 ms more.** Part B's cost is
therefore almost all a fixed, once-per-process charge paid the first time a file
rule runs, plus a small per-file charge: 90 plus 1846 times 0.35 is about 730
ms, and teyji's 190 ms gap is 90 plus 207 times 0.35, which is 162 ms. Both
match.

The fixed charge is `secrets::patterns::compiled()`. It builds **160 individual
`Regex` values and then a 160-pattern `RegexSet` over the same 160 patterns**,
behind a `OnceLock`. Once per process is the right granularity for a long-lived
server and the wrong one for a CLI: every `locrin check` is a new process, so a
pre-commit hook pays the whole 90 ms on every commit, and it pays it whether or
not any file holds anything a secret pattern could match.

The obvious repair, not made here because Task 16 changes rule logic only where
the gate forces a default: keep the `RegexSet`, which is the cheap membership
test the rule runs first anyway, and compile the individual `Regex` for a
pattern only once the set says that pattern matched. The rule's own module doc
already describes the set as the fast path.

With that fixed the warm single-file gate returns to about 230 ms and the cold
scan drops by roughly 90 ms, which leaves the cold gate needing the machine
question answered separately. Both are carried as concerns 2 and 3 of the Task
16 report, with the targets untouched.

#### Re-measured after the fix wave, with the same-session control

The repair above was made (`8c79f53`): the `RegexSet` is still built once per
process and each individual `Regex` now sits behind its own `OnceLock` and
compiles on the first line that matches its pattern. Re-measured at `8c79f53`
with the whole four-run method repeated at the branch base `8fe38e0` in a
detached worktree in the same session, machine confirmed idle beforehand (`0`
cargo, rustc or locrin processes), bench repository `<home>/fasting-app`,
fresh temporary cache per benchmark.

| Benchmark | Target | `8fe38e0` (base), four runs | `8c79f53` (HEAD), four runs |
| --- | --- | --- | --- |
| cold index | 5000 ms | 9382 / 9101 / 9162 / 9078 | 10294 / 8914 / 9905 / 9967 |
| warm single-file | 300 ms | 150 / 162 / 149 / 4268 | 179 / 182 / 178 / 184 |
| warm 30-file | 1000 ms | 224 / 228 / 229 / 226 | 317 / 306 / 294 / 319 |
| startup | 50 ms | 18 / 18 / 18 / 24 | 19 / 21 / 22 / 23 |

Discarding each side's first run, which is the method Part A established (the
first cold run after a build pays the page cache), and setting aside the base's
single 4268 ms warm outlier as a machine hiccup with three 149 to 162 ms
neighbours:

| Benchmark | Base mean | HEAD mean | Part B costs | Ruling |
| --- | --- | --- | --- | --- |
| cold index | 9114 ms | 9595 ms | **+481 ms, +5.3 percent** | inside the 10 percent allowance |
| warm single-file | 156 ms | 181 ms | +25 ms | **179 to 184 ms against 300 ms: green** |
| warm 30-file | 228 ms | 309 ms | +81 ms | 294 to 319 ms against 1000 ms: green |
| startup | 20 ms | 22 ms | +2 ms | green |

**The warm single-file gate is green again.** It measured 319 to 362 ms before
the repair and 178 to 184 ms after it, so the lazy compile removed about 145 ms
from the first file of a run, more than the 90 ms the isolation predicted. The
30-file check moved with it, 442 to 504 ms down to 294 to 319 ms.

**The cold gate is red, and the whole of the red is the machine.** The base
commit contains none of Part B and it measures **9078 to 9382 ms today** against
the 5062 to 5459 ms Task 16 recorded for that same commit and the 3750 to 3790
ms Part A recorded for it. That is the same measuring-box drift this ledger has
now recorded five times, and today it is at its worst: the base misses the
5000 ms target on its own by about 4100 ms. Part B's own share is +481 ms, or
5.3 percent, which is inside the ruling's 10 percent allowance for cold. The
target was not lowered and the absolute miss is recorded here beside the control
that explains it. A cold number worth a verdict needs a quiet box, and this one
is not it today.

#### Re-measured on a quiet box, 2026-09-10

The ruling above left the cold gate red with the whole of the red attributed to
the machine, and asked for a re-measurement on a quiet box. Re-measured on
`c26c1e6` (main, PRs #6 and #7 merged), `cargo test --release -p locrin-cli --
--ignored --nocapture` run four times in sequence, bench repository
`<home>/fasting-app`, fresh temporary cache per benchmark, machine
confirmed idle beforehand (`0` cargo, rustc or locrin processes, CPU load 0
percent).

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Run 4 | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | **25938 ms** | 3815 ms | 3810 ms | 3787 ms | PASS |
| warm single-file check | under 300 ms | 171 ms | 187 ms | 171 ms | 179 ms | PASS |
| warm 30-file check | under 1000 ms | 271 ms | 271 ms | 290 ms | 269 ms | PASS |
| startup (`--help`) | under 50 ms | 18 ms | 19 ms | 19 ms | 20 ms | PASS |

**Every gate is green, and the cold number is back where Part A left it.** Runs
2 to 4 sit inside 28 ms of each other at 3787 to 3815 ms, against Part A's 3750
to 3790 ms for code without Part B in it, so Part B's cold cost on a quiet box is
inside 65 ms, well under the +481 ms the loaded-box control measured and far
inside the 10 percent allowance. The 9078 to 9967 ms of the previous section
was the machine, as that section argued, and the target stands at 5000 ms with
about 1200 ms of headroom.

Run 1 is the method's discarded first run and it is reported because it is the
largest such number this ledger holds: **25.9 seconds**, with warm numbers on the
same invocation (171 / 271 / 18 ms) that are as fast as any run after it, so the
machine was not loaded. It is the operating system reading 1846 files off disk
after the checkout had been evicted from the page cache overnight. That is not a
benchmark of the engine, but it is what a person sees the first time they run
`locrin scan` on a large repository after a reboot, and a first impression of 26
seconds is worth a line in plan 4's `init` design (a progress line on the first
scan, or a file-count-and-elapsed notice on any scan over a few seconds) rather
than a lowered target. Carried as a follow-up, not a gate.

#### Registry

`all_rules()` lists **21** ids, asserted in order by `crates/rules/src/lib.rs`,
and the SARIF `tool.driver.rules` array carries **21** entries, asserted end to
end at `crates/cli/tests/cli.rs:777`. 16 file rules and 5 graph rules. Turning
`injection-sink` off changes neither number, by design: a rule that is off is
still described.
