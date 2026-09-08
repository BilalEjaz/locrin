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
