# Locrin engine: import graph and five rules (plan 2 part A) precision check

Spec 4.1 gate, measured against the FastLift checkout at `<home>/fasting-app`
as the corpus substitute. Release binary, fresh `LOCRIN_CACHE_DIR`, no `locrin.toml`
in the target repository, so this measures out-of-the-box precision with default
entry points and no configured boundaries.

Run: `locrin check` (terminal reporter, uncapped) on 1824 indexed files.
Verdict: BLOCK, 820 findings, 6 high, 166 medium, 648 low, 20836 ms.

## Result at a glance

| Rule | Findings on FastLift | Sampled | True positives | Gate (17/20) |
| --- | --- | --- | --- | --- |
| unused-import | 48 | 20 | 20/20 | PASS |
| dead-export | 596 | 20 | 20/20 | PASS |
| dead-file | 69 | 20 | 6/20 | FAIL |
| unreachable | 0 | 0 | not measurable | no findings |
| boundary-violation | 0 | 0 | not measurable | no findings |

`unreachable` and `boundary-violation` produced nothing on this repository.
`boundary-violation` cannot fire without a `[[boundaries]]` block in `locrin.toml`
and FastLift has no config file, so zero is the correct output rather than a miss.
`unreachable` finding nothing in 1824 files is plausible for a TypeScript codebase
under ESLint, but it means the rule is unmeasured by this corpus.

## Edge resolution across the repository

`SELECT resolution, count(*) FROM edges GROUP BY resolution`:

| Resolution | Count |
| --- | --- |
| resolved | 10454 |
| external | 5058 |
| unresolved | 1 |

One unresolved edge in 15513. The resolver is not the reason for anything below,
which is worth stating plainly: a low `dead-export` precision is usually blamed on
unresolved imports and that explanation is not available here.

Parse status: 1819 files `ok`, 5 files `error`. The five are `app/settings.tsx`,
`src/domain/food/collapseFoodDuplicates.ts`, `src/domain/sync/quarantineNotice.ts`,
`src/features/today/YourNumbersPanel.tsx`, `src/theme/historyPort.guard.test.tsx`.
This small number matters more than it looks: see the `dead-file` table, row 12.

## unused-import: 20/20 true positives (48 total)

Verified by reading every occurrence of the bound name in the importing file. In
every case the only occurrence was the import itself, or the other occurrences were
inside comments or string literals. JSX element usage is detected correctly: the
same import statements carry `Text`, `View` and `ScrollView` that the rule does not
flag, so a `<View>` in the body does count as a use.

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| app/(tabs)/food.tsx | 147 | true | `fmtDuration` appears only on the import line |
| app/(tabs)/index.tsx | 17 | true | `GlassCard` appears only on the import line |
| app/(tabs)/index.tsx | 20 | true | `alpha` appears only on the import line |
| app/(tabs)/workout.tsx | 23 | true | `SectionList` used only in three prose comments |
| app/circuit/custom.test.tsx | 110 | true | `customRemaining` appears only inside the import list |
| app/coach/parqReturnTo.guard.test.tsx | 28 | true | `PARQ_QUESTIONS` appears only on the import line |
| app/food/scan.tsx | 49 | true | type-only import `NewFood` never referenced |
| app/group/[groupId]/compose.tsx | 13 | true | `TextInput` used only in a comment above the import |
| app/group/[groupId]/rename.tsx | 8 | true | `TextInput` appears only on the import line |
| app/group/composerSegments.guard.test.tsx | 51 | true | `alpha` appears only in a comment and inside a regex literal |
| app/group/create.tsx | 24 | true | `TextInput` appears only on the import line |
| app/group/join.tsx | 30 | true | `TextInput` appears only on the import line |
| app/group/limitRejection.guard.test.tsx | 59 | true | `groupCopy` unused, sibling `GROUP_COPY` is the one used |
| app/group/limitRejection.guard.test.tsx | 60 | true | `GROUP_TOTAL_CAP` appears only on the import line |
| app/group/limitRejection.guard.test.tsx | 60 | true | `GROUP_OWNED_CAP` appears only on the import line |
| app/group/mile-board.tsx | 17 | true | `TextInput` appears only on the import line |
| app/onboarding/disclaimer.tsx | 30 | true | `View` appears only on the import line |
| app/team/join.tsx | 9 | true | `TextInput` appears only on the import line |
| app/team/name.tsx | 9 | true | `TextInput` appears only on the import line |
| app/wearables/index.tsx | 10 | true | `Pressable` appears only on the import line |

Summary: 20/20 true positives.

## dead-export: 20/20 true positives (596 total)

Verified by searching every source file in the repository (excluding
`node_modules`, build output and `.planning`) for the exported name. No file other
than the declaring file mentions any of the twenty, so no import of any kind
reaches them, type-only imports included.

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| src/components/AlertSheet.tsx | 53 | true | `AlertSheetActionKind` named nowhere else in the repository |
| src/components/AlertSheet.tsx | 62 | true | `AlertSheetProps` named nowhere else |
| src/components/DisclosureRow.tsx | 17 | true | `DisclosureRowProps` named nowhere else |
| src/components/FadePressable.tsx | 57 | true | `splitLayoutStyle` appears elsewhere only inside a comment |
| src/components/FadePressable.tsx | 110 | true | `FadePressableProps` named nowhere else |
| src/components/FormErrorText.tsx | 19 | true | `FormErrorTextProps` named nowhere else |
| src/components/GlassDoor.tsx | 25 | true | `GlassDoorProps` named nowhere else |
| src/components/InfoSheet.tsx | 46 | true | `InfoSheetOption` named nowhere else |
| src/components/InfoSheet.tsx | 52 | true | `InfoSheetProps` named nowhere else |
| src/components/KeyboardSafeSheetBody.tsx | 25 | true | `KeyboardSafeSheetBodyProps` named nowhere else |
| src/components/PremiumLock.tsx | 150 | true | `PremiumLockProps` named nowhere else |
| src/components/QuestionHero.tsx | 31 | true | `QuestionHeroProps` named nowhere else |
| src/components/RollFigure.tsx | 30 | true | `FIGURE_SIZE` elsewhere is a separate local const in FoodEmptyState.tsx |
| src/components/RollFigure.tsx | 31 | true | `FIGURE_LINE` named nowhere else |
| src/components/RollFigure.tsx | 34 | true | `ROLL_MS` named nowhere else |
| src/components/ScreenHeader.tsx | 50 | true | `ScreenHeaderProps` named nowhere else |
| src/components/SecondaryButton.tsx | 47 | true | `SecondaryButtonProps` named nowhere else |
| src/components/SelectableCard.tsx | 24 | true | `SelectableCardProps` named nowhere else |
| src/components/SheetPanel.tsx | 45 | true | `SheetPanelProps` named nowhere else |
| src/components/StatCard.tsx | 20 | true | `StatCardProps` named nowhere else |

Summary: 20/20 true positives.

Output order put all twenty in `src/components`, which is a narrow slice of a rule
that fires 596 times, so a supplementary sample of ten was taken at even intervals
across the whole list (indices 40, 90, 150, 220, 300, 380, 450, 520, 560, 590):
`BlockDecisionInput`, `LedgerEventKind`, `HistoryPage`, `RunLogParse`,
`ReplaceExerciseSheetProps`, `ChallengeReportInputs`, `DayBandHandlers`,
`MileReport`, `GroupLeaveGate`, `addDaysIso`. All ten are named nowhere outside
their declaring file: 10/10 true positives, spread across `src/domain`,
`src/features` and `supabase/functions/_shared`.

One volume observation rather than a precision one: 111 of the 596 are exported
React `Props` types that the component's own file is the only consumer of. The
claim is correct and the suggested fix (drop the `export` keyword) is safe, but it
is a large, low-value block of findings for an application repository. Worth a
founder decision on whether a component's own `Props` type deserves its own
treatment, not a change I would make unasked.

## dead-file: 6/20 true positives (69 total)

This is the rule that fails the gate.

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| .planning/releases/1.7.0-voice/voice-ai-probe.mjs | 1 | false | Hand-run script; its own header documents `node .planning/releases/1.7.0-voice/voice-ai-probe.mjs` |
| assets/brand/gen.js | 1 | false | Hand-run brand asset generator; `src/components/Logo.tsx` cites it as the geometry source of truth |
| components/external-link.tsx | 1 | true | Expo template leftover, named nowhere in the repository |
| components/haptic-tab.tsx | 1 | true | Expo template leftover, named nowhere in the repository |
| components/hello-wave.tsx | 1 | true | Expo template leftover, named nowhere in the repository |
| components/ui/icon-symbol.ios.tsx | 1 | true | Expo template leftover, named nowhere in the repository |
| components/ui/icon-symbol.tsx | 1 | true | Expo template leftover, named nowhere in the repository |
| jest-setup-masked-view.js | 1 | false | Loaded by Jest: `package.json` `jest.setupFiles` names `<rootDir>/jest-setup-masked-view.js` |
| plugins/withHealthConnectManifest.js | 1 | false | Expo config plugin: `app.json` `plugins` names `./plugins/withHealthConnectManifest` |
| plugins/withLargeScreenCompat.js | 1 | false | Expo config plugin: `app.json` `plugins` names `./plugins/withLargeScreenCompat.js` |
| src/features/history/LogbookTrace.tsx | 1 | true | Superseded component, no importer and no JSX usage anywhere |
| src/services/quarantineRetry.ts | 1 | false | Really imported: `app/settings.tsx` L169 `import { retryQuarantinedRows } from '../src/services/quarantineRetry'`. The edge is missing because `app/settings.tsx` has `parse_status = 'error'` and the recovery dropped that import |
| supabase/functions/challenge-create/index.ts | 1 | false | Deployed Deno edge function, invoked over HTTP by name from `src/services/group/client.ts` and callers |
| supabase/functions/challenge-end/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/challenge-join/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/challenge-leave/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/challenge-list/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/challenge-progress/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/challenge-report-day/index.ts | 1 | false | Deployed Deno edge function, invoked by name |
| supabase/functions/coach-week/index.ts | 1 | false | Deployed Deno edge function, invoked by name |

Summary: 6/20 true positives.

### The dominant false-positive pattern

Thirteen of the fourteen false positives are one thing: an entry point that a
runtime, a build tool or a person loads by a path the import graph cannot see.
They break into four kinds.

1. Deployed serverless functions (8 of 20). `supabase/functions/<name>/index.ts` is
   deployed by directory name and called over HTTP. Nothing imports it and nothing
   ever will.
2. Config strings (3 of 20). `app.json` `plugins` names the two Expo config plugins,
   `package.json` `jest.setupFiles` names the Jest setup file. The paths are there,
   in JSON, in the repository root, and `EntryPoints::detect` does not read them.
3. Hand-run scripts (2 of 20). `assets/brand/gen.js` and the voice probe are run by
   a person with `node <path>`. `DEFAULT_ENTRY_GLOBS` already covers `**/scripts/**`
   and `**/bin/**`, so this is the same idea applied to files that happen to live
   elsewhere.
4. A missing edge from a parse failure (1 of 20). `src/services/quarantineRetry.ts`
   is genuinely imported. The importer, `app/settings.tsx`, failed to parse, so two
   consecutive import statements never reached the `edges` table. The engine
   correctly refuses to report findings from a file that failed to parse, but it
   still trusts that file's partial edge set when judging other files.

Point 4 is the one worth calling out as an engine defect rather than a modelling
gap. Five files in this repository have `parse_status = 'error'` and one of them
was enough to declare a live module dead. Any file whose parse failed should be
treated as an unknown importer by the graph rules rather than as a file with no
imports, however few such files there are.

Points 1 to 3 are all "the entry point set is too narrow for a real repository".
They are fixable without touching the rule's logic (more default entry globs,
reading `app.json` plugins and `package.json` jest fields), and the rule's own fix
text already tells the user to add `entry_points` to `locrin.toml`. That is a
product decision, not one this task takes: no rule confidence, severity or
enablement has been changed.

## Benchmarks (spec 3.4, release build)

Run: `cargo test --release -p locrin-cli -- --ignored --nocapture`, against the same
FastLift checkout, 1824 files.

| Benchmark | Target | Measured | Result |
| --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 10038 ms | FAIL |
| warm single-file check | under 300 ms | 623 ms | FAIL |
| startup (`--help`) | under 50 ms | 27 ms | PASS |

An earlier run of the same binary on the same machine gave 12468 ms, 769 ms and
47 ms, so run-to-run variance on this Windows box is roughly 25 per cent. It does
not change either verdict.

### Where the time goes

The plan named three suspects. All three were checked and all three are already
correct in the code, so none of them is the cause.

- `probe` does not touch the disk for a bare specifier. `Resolver::resolve` returns
  `External` for any bare specifier no alias or workspace owns, before any
  filesystem call.
- `EntryPoints::detect` and `Resolver::new` are each built once per run in
  `full_findings`, not per file.
- `symbols::extract` is called once per file in `indexer::record`. It is called
  again by `enclosing_symbol` inside `anchor_for`, but that is once per finding,
  not once per file.

Measured breakdown instead, by instrumenting `index_files` temporarily and then
reverting the instrumentation (a cold `scan` of 1819 files, wall clock 10.3 s):

| Phase | Time |
| --- | --- |
| read plus content hash, every file | 545 ms |
| tree-sitter parse, every file | 4590 ms |
| record (symbols, edges, allow lines, file row) | 4128 ms |
| walk, resolver build, index open, prune, process teardown | about 1000 ms |

So the cold index is parse plus database write, roughly half each, both entirely
single threaded. `indexer::record` opens four separate transactions per file, which
is about 7300 commits for this repository.

The warm single-file check splits differently. Of 623 ms, about 390 ms is inside
`check` and the rest is process start and exit. Inside that 390 ms, 170 ms is
reading and content-hashing all 1819 files to decide which changed (the named file
is the only one parsed, at 1 ms), and the remainder is the walk, the index open,
the graph rules and the prune.

The two honest routes to the targets are parallel parsing and one batched
transaction per run for the cold index, and a cheaper change check (file size and
mtime before content hash) plus avoiding the full walk for the warm check. Both are
real work on `crates/core`, not a local fix inside this task's files, and neither
target was raised.

## dead-file re-measure after Task 15b

Task 15b made a file that failed to parse an unknown importer rather than an empty
one, and widened the entry-point set (serverless functions, Expo config plugins,
`package.json` `jest` setup fields, `app.json` plugins).

FastLift is a live repository and the founder worked in it the same day, so the
run at the top of this document is not a like-for-like baseline for the numbers
below. To keep the comparison honest, both binaries were run against the tree as
it stands now, each with a fresh `LOCRIN_CACHE_DIR`: the Task 15b binary, and one
built from `baac531`, the commit the first measurement used. Corpus drift is the
whole explanation for `unused-import` reading 48 above and 57 in both runs here.

| Rule | baac531 | after Task 15b | Change |
| --- | --- | --- | --- |
| dead-file | 68 | 9 | -59 |
| dead-export | 598 | 516 | -82 |
| unused-import | 57 | 57 | none |
| leftover-commented-code | 98 | 98 | none |
| leftover-debug | 6 | 6 | none |
| leftover-agent-marker | 4 | 4 | none |
| Total | 831 | 690 | -141 |

Verdict is BLOCK either way: 6 high, 166 medium, 659 low becomes 6 high, 107
medium, 577 low. Wall clock is unchanged, measured by alternating warm runs:
9626 and 9717 ms for the new binary against 9643 and 9709 ms for `baac531`. The
very first run of the new binary reported 40678 ms, which was a cold filesystem
cache and not the change.

### The nine remaining dead-file findings

There are nine, not twenty, so every one of them is listed rather than sampled.

| File | Verdict | Reason |
| --- | --- | --- |
| .planning/releases/1.7.0-voice/voice-ai-probe.mjs | false | Hand-run probe; its own header documents `node .planning/releases/1.7.0-voice/voice-ai-probe.mjs` |
| assets/brand/gen.js | false | Hand-run brand asset generator; `src/components/Logo.tsx` cites it three times as the geometry source of truth |
| components/external-link.tsx | true | Expo template leftover; neither the path nor `ExternalLink` appears anywhere else in the repository |
| components/haptic-tab.tsx | true | Expo template leftover; `HapticTab` appears nowhere else |
| components/hello-wave.tsx | true | Expo template leftover; `HelloWave` appears nowhere else |
| components/ui/icon-symbol.ios.tsx | true | Expo template leftover; `IconSymbol` appears nowhere else |
| components/ui/icon-symbol.tsx | true | Expo template leftover; `IconSymbol` appears nowhere else |
| src/features/history/LogbookTrace.tsx | true | Superseded by `DurationTrend`; no importer and no JSX usage. Two theme guard tests name the path as a string, but they read the file off disk rather than load the module |
| workers/media/src/worker.ts | false | Deployed Cloudflare Worker: `workers/media/wrangler.toml` sets `main = "src/worker.ts"`, and `src/features/coach/exerciseMedia.ts` calls it over HTTP at its workers.dev hostname |

Summary: 6/9 true positives, 67 per cent against a gate of 85 per cent (17/20).
The gate is not cleared, so per the task brief no rule confidence, severity or
enablement has been changed.

The count of false positives fell from an estimated 48 of 68 to 3 of 9, but the
rate did not, because the fix removed whole families of false positives and left
the long tail behind. A rate gate on a shrinking population is a harder gate.

### What the remaining three have in common

All three are the same modelling gap as before, an entry point named somewhere the
import graph does not read, and none is a graph defect.

1. A config format the engine does not parse. `wrangler.toml` names the Worker's
   main file, relative to the directory the config sits in. `EntryPoints::detect`
   reads JSON only.
2. Hand-run scripts, again. `voice-ai-probe.mjs` and `assets/brand/gen.js` are run
   by a person with `node <path>` and live under neither `scripts/` nor `bin/`.

Both are answerable by more defaults (`**/wrangler.toml` `main`, or a wider script
convention) and both are already answerable today by `entry_points` in
`locrin.toml`, which is exactly what the rule's fix text tells the user.

### The parse-failure fix cannot be re-confirmed on this corpus

The first measurement pinned `src/services/quarantineRetry.ts` on `app/settings.tsx`
failing to parse and losing two consecutive import statements. That file has since
been edited: the import the report cites at L169 now sits at L171. On the tree as
it stands, tree-sitter recovers every specifier in it. The `edges` table holds 92
distinct specifiers from `app/settings.tsx` and the file's text contains exactly
92, and both binaries record the same 167 edges from it. Of the other four files
with `parse_status = 'error'`, two have no imports at all
(`collapseFoodDuplicates.ts`, `quarantineNotice.ts`) and two lose none.

So the text fallback runs on this repository and adds nothing to it, and none of
the 59 removed `dead-file` findings is attributable to it: all 59 are entry
points, 56 under `supabase/functions`, the two `plugins/` files, and
`jest-setup-masked-view.js`. The defect is real and is covered by tests
(`crates/core/src/imports.rs` and the `dead_file/parse_error` fixture, which
reproduces the exact failure), but this corpus at this commit can neither confirm
nor refute it.

### dead-export moved as expected

598 to 516, a drop of 82, and every one of the 82 sits under
`supabase/functions/_shared` (78 directly, 4 in `_shared/wearableProviders`). That
is the entry-point widening doing what it should for the deployed handlers
themselves, but it is wider than the evidence asks for, and that is worth saying
plainly.

All 56 `supabase` `dead-file` findings that the fix removed were
`supabase/functions/<name>/index.ts`, every single one, and the other new glob
`**/functions/*/index.*` already matches all of them. `supabase/functions/**` adds
nothing to the `dead-file` result and only exempts `_shared`, which no
`dead-file` finding ever named because the handlers do import it. What it does
cost is `dead-export` recall: three of the ten supplementary samples above that
were read and verified as true positives (`MileReport`, `GroupLeaveGate`,
`addDaysIso`) are among the 82 now silenced.

On this corpus, dropping `supabase/functions/**` and keeping only
`**/functions/*/index.*` would give the same nine `dead-file` findings and keep 82
`dead-export` findings that a hand check found correct. The task brief specified
the wide glob, so it is what shipped; this note is the measurement a founder would
want before deciding whether to narrow it. No `dead-export` finding was added.

`unused-import` is untouched at 57, which is correct: it is a file-scope rule that
reads neither edges nor entry points.

### After review

Both blanket globs were dropped on the founder's ruling: `supabase/functions/**`
because `**/functions/*/index.*` already matches every `dead-file` finding it
removed while the wide form silenced 82 hand-verified true `dead-export` findings
under `_shared`, and `plugins/**` because `app.json` already names the Expo
plugins that are entry points and a root `plugins/` directory of application code
would otherwise have its dead files hidden. `wrangler.toml`'s `main` was added as
an entry source, which answers the one remaining false positive that was not a
hand-run script, and `dead-file` now ships disabled by default (opt in with
`[rules.dead-file]` `enabled = true`) until a repository's `entry_points` are
curated, because 6/9 is below the spec 10.2 precision gate and every remaining
miss is an entry point named outside JavaScript. These figures were not
re-measured on FastLift.

## Benchmarks after Task 15c

Same command, same FastLift checkout, same machine: `cargo test --release -p
locrin-cli -- --ignored --nocapture`, run three times back to back. All three
benchmarks pass on all three runs.

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 2589 ms | 2313 ms | 2347 ms | PASS |
| warm single-file check | under 300 ms | 254 ms | 246 ms | 252 ms | PASS |
| startup (`--help`) | under 50 ms | 25 ms | 26 ms | 25 ms | PASS |

The starting point measured immediately before the work, on the same machine,
was 4731 / 4371 / 4815 ms cold, 336 / 332 ms warm and 27 / 28 / 26 ms startup.
That cold figure is well under the 10038 ms this document recorded earlier;
the difference is the operating system's file cache, which was cold for the
earlier measurement and warm for these. The warm check failed its target in
both states, before and after that difference.

### What each change bought

| Step | Cold | Warm |
| --- | --- | --- |
| before | 4731 ms | 336 ms |
| one transaction per run | 3981 ms | 306 ms |
| parallel parsing | 2539 ms | 340 ms |
| stat before read | 2630 ms | 257 ms |

Parallel parsing does nothing for the warm check, which parses one file, and the
stat check does nothing for a cold scan, which has no stored stats to compare
against. Each target needed its own fix.

### Where the warm check's remaining time goes

Measured by instrumenting `full_findings` temporarily and reverting the
instrumentation, on a warm cache, `check app/_layout.tsx` over 1819 files:

| Phase | Time |
| --- | --- |
| walk | 106 ms |
| stat plus index lookup, every file, then read the one named file | 47 ms |
| rules | 46 ms |
| index open | 7 ms |
| config, resolver, entry points, prune, commit | about 2 ms |
| process start and exit, baseline load, reporting | about 40 ms |

The walk is now the largest single item and the obvious next lever: `walk.rs`
uses `WalkBuilder::build_parallel`'s single-threaded sibling, `build`. Nothing
was changed there, because all three targets pass without it and the plan's
instruction was not to parallelise the walk unless the warm target needed it.
It is worth revisiting: the warm check clears its target by about 45 ms, which
is a real but not generous margin.

### After the Task 15c review fixes

The review of Task 15c found four Important issues (the stat taken on the wrong
side of the read, a deferred `BEGIN` that turns a concurrent run into a failure,
a touched-but-identical file never getting its stat refreshed, and a
second-granularity modification time). All four are fixed, so the numbers above
no longer describe the code that ships. Same command, same FastLift checkout,
same machine, three runs back to back:

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 2500 ms | 2474 ms | 2549 ms | PASS |
| warm single-file check | under 300 ms | 281 ms | 268 ms | 272 ms | PASS |
| startup (`--help`) | under 50 ms | 31 ms | 31 ms | 30 ms | PASS |

The warm check is the one to watch. Two earlier attempts at these runs measured
335 ms and 302 ms, both failures, so the same benchmark was measured four times
at this commit and four times at the commit before the fixes, interleaved to
cancel drift: 270 / 254 / 261 / 258 ms with the fixes against 285 / 289 / 292 /
263 ms without them. The fixes are not what makes this benchmark flake; a busy
machine is. The margin is about 30 ms and the walk is still 106 ms of the total,
so `WalkBuilder::build_parallel` remains the lever if the margin is judged too
thin.

### After the parallel walk

The lever named twice above was pulled: `walk_with_stats` now uses
`WalkBuilder::build_parallel` over `available_parallelism()` capped at 8, with
matches collected into a shared vector and sorted at the end, so the file list is
byte-identical to the sequential walk's. Same command, same FastLift checkout,
same machine, three runs back to back:

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 3171 ms | 2418 ms | 2522 ms | PASS |
| warm single-file check | under 300 ms | 175 ms | 196 ms | 243 ms | PASS |
| startup (`--help`) | under 50 ms | 35 ms | 29 ms | 30 ms | PASS |

The warm check's margin goes from about 20 to 30 ms to about 57 to 125 ms, which
is roughly the 106 ms the walk was costing, less what the pool cannot remove. The
spread across the three runs is still the machine: run 3's 243 ms is the same
loaded-box noise that produced the 302 ms and 335 ms failures recorded above, but
it now lands inside the target rather than outside it.

## Benchmarks after Task 16b

Task 16 made `scan` warm the findings cache, which meant the file rules had to
run over every parsed file rather than over the changed ones alone. Five rules
over about 1,840 parsed files, single-threaded, cost about 3.7 s, and the cold
`scan` benchmark went to about 7.8 s against a 5000 ms target.

Task 16b parallelises that pass. `run_file_rules` gives each parsed file a
`RuleContext` of its own and runs the file rules over it across the rayon pool,
collecting per-file results and flattening them in file order, so the output is
identical after the reporter's sort (file-major versus rule-major before it) on
every machine. The per-file contexts
carry no index (`RuleContext.index` is now `Option<&Index>`), because no file
rule reads one; a graph rule takes it through `ctx.index()?`, which errors with
"graph rule run without an index" rather than panicking. The graph rules still
run once, through `run_rules`, over the whole index.

Same command, same FastLift checkout, same machine, three runs back to back:

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 3347 ms | 3024 ms | 2996 ms | PASS |
| warm single-file check | under 300 ms | 159 ms | 159 ms | 152 ms | PASS |
| startup (`--help`) | under 50 ms | 19 ms | 23 ms | 18 ms | PASS |

The cold run is back inside the target with about 1650 ms of margin at its
worst, and it now leaves a warm findings cache behind it, which the numbers
before Task 16 did not. The warm check is unchanged by this task, as expected:
it parses one file, so there is nothing for the pool to spread.

## Benchmarks after Task 19

Task 19 adds the fourth spec 3.4 benchmark, `warm_thirty_file_check_under_one_second`,
which stands in for a typical pull request: a warm cache, then one `check` invocation
over 30 source files from the FastLift checkout's `app/` tree. The listing takes the
top level of `app/` first and then each immediate subdirectory in turn, sorted, and
truncates at 30, because the top level alone holds only 11 files. Same command as the
earlier tables (`cargo test --release -p locrin-cli -- --ignored --nocapture`), same
checkout, same machine, three runs back to back. All four benchmarks pass on all
three runs.

| Benchmark | Target | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 3274 ms | 2962 ms | 3038 ms | PASS |
| warm single-file check | under 300 ms | 144 ms | 147 ms | 145 ms | PASS |
| warm 30-file check | under 1000 ms | 191 ms | 197 ms | 192 ms | PASS |
| startup (`--help`) | under 50 ms | 18 ms | 19 ms | 19 ms | PASS |

The warm single-file check is faster than the part A number it is compared against
(175 / 196 / 243 ms after the parallel walk, and 254 / 246 / 252 ms before the review
fixes): 144 to 147 ms here, a spread of 3 ms across three runs rather than 68 ms.
That is the findings cache doing what Task 16 built it for. A warm run no longer
re-parses and re-evaluates the whole repository, so the cost of a narrowed check is
dominated by the walk and the cache reads rather than by work proportional to
repository size.

The 30-file number is the point of the new benchmark. It costs 191 to 197 ms against
a single file's 144 to 147 ms, so 29 extra files add about 48 ms in total, roughly
1.7 ms each, against a per-invocation floor of about 145 ms. The target is 1000 ms
and the measured worst run uses 20 percent of it. The floor, not the per-file cost,
is what a future optimisation would have to attack: at this marginal rate a 100-file
pull request would still land near 315 ms.

An earlier revision of this benchmark listed only the top level of `app/`, measured
11 files, and recorded the shortfall as a caveat. The listing was widened one
directory level down in the Task 17/18 review pass, so the benchmark now measures the
30 files the spec names, and the numbers above replace the 11-file table. The
extrapolation made from the 11-file run (185 to 200 ms for 30 files) held.
