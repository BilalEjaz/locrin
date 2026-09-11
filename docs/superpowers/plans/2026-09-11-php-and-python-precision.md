# Locrin engine: PHP and Python (plan B) precision check

Spec 10.2 gate for the five rules that run on PHP and Python (`leftover-debug`,
`leftover-commented-code`, `leftover-agent-marker`, `secret-exposed` and
`vulnerable-dependency`), measured once per rule-language pair on one real
repository per language. Every other rule is filtered structurally by
`Rule::languages` and produces nothing on PHP or Python files by construction,
so it has no pair to measure.

Binary: `target/release/locrin.exe` 0.4.0, built from `engine/languages` at
`b71846f` (Tasks 1 to 7). The `enabled_for` mechanism this document's gate
feeds was added afterwards on the same branch and changes no finding: it
defaults to `true` for every rule and every language, and nothing below turns
it off.

## Corpora

| Corpus | Language | Source | Commit | Indexed files |
| --- | --- | --- | --- | --- |
| BookStack | PHP | `github.com/BookStackApp/BookStack`, tag `v26.05.4` (latest release, 2026-08-24), `git clone --depth 1` | `cec78b1b` | 2152 (1773 `.php`, 389 JavaScript and TypeScript, 2 excluded for parse errors) |
| FastSpot | Python | `<home>/fastspot` (the founder's Kraken paper-trading bot) | working tree, copied 2026-09-11 | 96 (all `.py`) |

Both were measured on a copy in the session scratchpad and not on the
original: FastSpot was copied with `.venv`, `venv`, `node_modules`, `.git` and
`.pytest_cache` left out, BookStack was cloned fresh. Each copy got a two-line
`locrin.toml` (`[languages]` with `php = true` or `python = true`) and its own
`LOCRIN_CACHE_DIR` under the scratchpad. Nothing was written into
`<home>/fastspot`, into the repository under review, or into any
`.git`.

Command per corpus: `locrin --root <copy> check --sarif-file <corpus>.sarif
--offline`, uncapped, on a fresh cache; then one online pass, `locrin --root
<copy> check --sarif-file <corpus>-online.sarif`, so `vulnerable-dependency`
could fetch advisories for BookStack's `composer.lock` and `package-lock.json`.
FastSpot has neither a `requirements.txt` nor a `poetry.lock` (its only
manifest is `pyproject.toml`, which the engine does not read), so the PyPI pair
is unmeasured on this corpus and the online pass found nothing to fetch.

| Corpus | Pass | Verdict | Findings (all rules) | Engine | Wall |
| --- | --- | --- | --- | --- | --- |
| BookStack | offline, fresh cache | BLOCK | 340 (66 high, 11 medium, 263 low) | 76056 ms | 76734 ms |
| BookStack | online, warm cache | BLOCK | 369 (87 high, 17 medium, 265 low) | 5780 ms | 6676 ms |
| BookStack | offline, warm cache (after the online pass) | BLOCK | 369 | 239 ms | 794 ms |
| FastSpot | offline, fresh cache | PASS | 0 | 578 ms | 1057 ms |
| FastSpot | online, warm cache | PASS | 0 | 64 ms | 558 ms |

The 29 findings the online pass adds are all `vulnerable-dependency` on
`package-lock.json` (21 High, 6 Medium, 2 Low), an npm pair plan 3 already
measured at 54/54; `composer.lock` produced none, and that zero is checked
below. The warm offline pass after it reports the same 369 because the online
pass left an advisory snapshot in the cache. The wall column is not the
performance number: see the run-time section and the re-measurement there.

Findings from the five rules, per corpus, on the files of the language under
measurement (BookStack's JavaScript findings are the engine's existing
behaviour and are not this gate's):

| Rule | BookStack (PHP files) | FastSpot (Python files) |
| --- | --- | --- |
| leftover-debug | 3 | 0 |
| leftover-commented-code | 3 | 0 |
| leftover-agent-marker | 2 | 0 |
| secret-exposed | 1 | 0 |
| vulnerable-dependency | 0 on `composer.lock` (Packagist) | no PyPI lockfile |

## The verdict standard

The same one the plan 2 and plan 3 reports used: a finding is true when a
maintainer reading it would change the code, and false when the correct answer
is "no, this is intentional and already decided". Where a label was close it
went to false, because an optimistic label costs more than a pessimistic one.

Sampling was the brief's: every finding when a pair has 20 or fewer, otherwise
20 drawn with `random.Random(11).sample` from the findings sorted by file then
line. No pair reached 20, so every finding below is labelled.

## Result at a glance

| Pair | Findings | Sampled | True | Gate (17/20, or 85 percent of at least 5) | Default |
| --- | --- | --- | --- | --- | --- |
| leftover-debug on PHP | 3 | 3 | 1/3 | unmeasured, sample too small | on |
| leftover-commented-code on PHP | 3 | 3 | 0/3 | unmeasured, sample too small | on |
| leftover-agent-marker on PHP | 2 | 2 | 2/2 | unmeasured, sample too small | on |
| secret-exposed on PHP | 1 | 1 | 0/1 | unmeasured, sample too small | on (locked) |
| vulnerable-dependency on Packagist | 0 | 0 | not measurable | unmeasured, zero probed | on |
| leftover-debug on Python | 0 | 0 | not measurable | unmeasured, zero probed | on |
| leftover-commented-code on Python | 0 | 0 | not measurable | unmeasured, zero probed | on |
| leftover-agent-marker on Python | 0 | 0 | not measurable | unmeasured, zero probed | on |
| secret-exposed on Python | 0 | 0 | not measurable | unmeasured, zero probed | on (locked) |
| vulnerable-dependency on PyPI | no lockfile | 0 | not measurable | unmeasured, rule never ran, path probed | on |

Ten pairs, nine findings between them, and not one pair reaches the five
findings the gate needs to say anything. Every pair ships on by the brief's
rule ("fewer than 5 findings is unmeasured, ships on"), and this document is
mostly about what the nine findings and the zeros do say.

## leftover-debug on PHP: 1 true of 3

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| app/Theming/ThemeViews.php | 52 | true | `dd($viewPath, $data);` inside `if (str_contains('book-tree', $viewPath))` in shipped application code. `dd` dumps and exits the request; the guard's arguments are also the wrong way round for `str_contains` (haystack `'book-tree'`, needle the view path), so it almost never fires, which is exactly what a forgotten debug branch looks like. A maintainer deletes both lines |
| tests/TestCase.php | 192 | false | `$toIncludeStr = print_r($mapToInclude, true);` with the second argument `true`, so `print_r` returns a string and prints nothing; the string is the failure message of the `assertThat` two lines down. The rule's claim, debug output, is not what this call does |
| tests/TestCase.php | 193 | false | The same shape on the next line |

### The false-positive pattern, and the fix

**`print_r` and `var_export` in return mode.** Both take a second argument
that, when true, returns the rendering instead of printing it, and in that mode
they are string formatters rather than debug output. The two false findings are
one use of that, feeding an assertion message inside the test base class. The
fix is contained: in `is_php_sink`, a `print_r` or `var_export` whose call
carries a second argument that is the literal `true` is not a sink. It is not
made here because Task 8 changes rule logic only where the gate forces a
default, and the gate cannot be forced by three findings. It is carried as a
follow-up, with the note that two of the three PHP findings this rule produced
on 1773 files are this shape, so at scale it is the class most likely to decide
the pair.

**Made in round two** (commit "rules: return-mode print_r and var_export are
not debug sinks"): `is_php_sink` now asks `php_return_mode`, which reads the
call's `arguments` node and answers true when the second positional argument,
or an argument named `return`, is the literal `true`. `print_r($x, true)`,
`var_export($x, true)` and `print_r($x, return: true)` are no longer findings;
`print_r($x)`, `print_r($x, false)` and `print_r($x, $flag)` still are, because
the last may print. The two TestCase.php findings above would not fire today.
Fixture lines for both forms sit in `tests/fixtures/leftover_debug/php`. No
migrations exemption for `leftover-commented-code` was made: the heuristic
already requires code shape, and the round-one migration block is scored as
the heuristic finds it.

The one true finding is worth a line on its own: a `dd()` call in the current
release of a widely deployed application, guarded by a condition that cannot
match its own arguments. That is the rule's reason to exist, found on the first
real PHP repository it read.

## leftover-commented-code on PHP: 0 true of 3

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| database/migrations/2015_08_31_175240_add_search_indexes.php | 18 | false | Four `//` lines of `DB::statement("ALTER TABLE ... ADD FULLTEXT ...")` under a two-line comment: "This was removed for v0.24 since these indexes are removed anyway and will cause issues for db engines that don't support such indexes." The block is commented-out code, the rule is right about that, and the decision to keep it is written directly above it, in a migration that is frozen history by convention. A maintainer answers "no" |
| database/migrations/2015_12_05_145049_fulltext_weighting.php | 18 | false | The same block and the same comment, in the next migration |
| database/migrations/2017_03_19_091553_create_search_index_table.php | 58 | false | The same block and the same comment, in the `down()` of a later migration |

### The false-positive pattern, and the fix

**A documented block in a frozen file.** All three are one decision made once
in 2016 and copied into three migration files, each with the sentence that
explains it. Two things separate it from the leftover the rule is for: the
comment immediately above says why the code is off, and the file is a migration,
which a Laravel project never edits after it has shipped. The first is hard to
read mechanically (any prose can sit above a block) and the second is easy: a
repository that treats `database/migrations/**` as history excludes it, and the
rule's Medium confidence already keeps these advisory rather than blocking. A
`commented_code_allowed` glob list with `**/migrations/**` as a default, the
shape `debug_allowed` already has, is the contained fix, and it is carried as a
follow-up rather than made here for the reason given above.

The pessimistic reading is the one recorded. The other reading, that a block of
executable SQL kept in comments for ten years is what the rule exists to name
and the explanatory comment is the reason version control holds the history,
would make all three true. The gate does not turn on it at three findings, and
the label follows the standard's instruction to prefer the pessimistic side.

## leftover-agent-marker on PHP: 2 true of 2

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| app/Entities/Controllers/PageRevisionController.php | 79 | true | `// TODO - Refactor PageContent so we don't need to juggle this`. A marker with no issue reference above code that is still juggling; the rule asks for a link or a resolution, and either is the right answer |
| app/Entities/Controllers/PageRevisionController.php | 112 | true | `// TODO - Refactor PageContent so we can de-dupe these steps`. The same |

Two markers in 1773 PHP files, and a `grep` for `TODO`, `FIXME`, `HACK` and
`XXX` over the same files finds exactly those two lines, so the rule read every
marker the corpus has and reported both.

## secret-exposed on PHP: 0 true of 1, and a STOP to record

| File | Line | Verdict | Reason |
| --- | --- | --- | --- |
| tests/Helpers/OidcJwtHelper.php | 92 | false | `Private key block credential: MIIE...(64 chars)`. An RSA private key returned by `privatePemKey()` in a test helper, used at line 66 to sign the JWTs the OIDC tests then verify against the public key returned by the method above it. It protects nothing outside the test suite, so "revoke the credential now" is not what a maintainer does. The rule's claim, that a private key block sits in the file, is accurate |

`secret-exposed` is locked (spec 4.3), which is why plan 3's report wrote that
"a false positive here is a STOP for the founder". This is one, by this
document's standard, and it is recorded as such rather than argued away: the
pair cannot ship off (the rule is locked and the sample is one), the finding is
High confidence and would block a BookStack run until it is baselined, and the
founder decides whether a private key in a test path whose public half sits in
the same file is a fixture the rule should learn or a finding a repository
should accept once with a reason. Both are defensible: a real key committed
under `tests/` would look the same to the second reading, and the first
reading would miss it. Nothing was changed pending that ruling.

The other reading, that any private key material in a repository is the rule's
to report and the baseline is where a fixture is accepted, would make this
finding true. It is the reading every general-purpose secret scanner takes.

## vulnerable-dependency on Packagist: 0 findings, and the zero was checked

BookStack's `composer.lock` holds 151 packages (113 production, 38 development)
and the online pass reported no advisory for any of them, while reporting 29
for the `package-lock.json` beside it. A zero from a rule that fetched and said
nothing is worth a probe from both sides, so it got two:

1. **The corpus.** All 151 name-and-version pairs were sent to OSV's own
   `querybatch` endpoint under the `Packagist` ecosystem, read-only, with the
   `v` prefix Composer writes stripped the way the engine strips it. OSV
   answered zero advisories for every one. A control query in the same session
   for `laravel/framework 10.0.0` answered four (`GHSA-5vg9-5847-vvmq`,
   `GHSA-78fx-h6xr-vch4`, `GHSA-crmm-hgp2-wgrp`, `GHSA-gv7v-rgg6-548h`), with
   and without the prefix, so the query method sees Packagist advisories when
   they exist. BookStack's lock is eighteen days old at the tag, on
   `laravel/framework v12.67.0`, and has none.
2. **The engine.** A scratch repository outside every corpus, holding a
   one-package `composer.lock` pinning `laravel/framework v10.0.0`, run online
   with its own cache: four `vulnerable-dependency` findings at line 1, the
   same four advisories OSV named, two of them High. So the Composer path
   parses, normalises, fetches and reports.

Unmeasured by the gate's definition, and it ships on with the zero probed from
both sides. One thing the probe surfaced that the gate does not score: every
Packagist and PyPI finding's fix line reads "Fixed version not read from this
advisory (its ranges are not in a form this engine orders); review <id> for the
version to move to". OSV publishes those two ecosystems' ranges as `ECOSYSTEM`
rather than `SEMVER`, which `b71846f` chose to say honestly rather than order.
The finding is right and the upgrade target is missing, which is the plan 3
`fast-uri` situation from the other side and a follow-up of the same size.

## vulnerable-dependency on PyPI: no lockfile, and the path was probed

FastSpot's only manifest is `pyproject.toml`. The engine reads
`requirements.txt` and `poetry.lock` for PyPI and neither exists, so the rule
never ran on this corpus and the pair is unmeasured with no finding in either
direction. So that the zero is not read as the rule's, the path was probed the
same way: a scratch `requirements.txt` holding `urllib3==1.26.4`, online, fresh
cache, reported 18 findings (6 High, 3 Medium, 9 Low), all against that
package, all naming the advisory. The next Python repository with a lockfile is
the measurement this pair still owes.

## The Python zeros: four rules, 96 files, nothing, and why

FastSpot produced no finding from any rule. Four pairs at zero is the gate's
"unmeasured, ships on", and it is also the kind of zero that needs to be shown
to be the corpus's and not the engine's. Three checks:

1. **The walk.** `locrin scan` on the copy reports `indexed 96 file(s)`, which
   is every `.py` under the tree with the excluded directories left out.
2. **The rules.** A scratch repository outside every corpus, `[languages]
   python = true`, holding one file with `import pdb`, `breakpoint()`, a
   GitHub-token-shaped assignment, a `# TODO` line and a four-line commented-out
   function: five findings at their real lines, one from each of the four rules
   (two from `leftover-debug`), the token masked as `ghp_...(42 chars)`, in
   102 ms.
3. **The corpus.** A `grep` over the 96 files finds no `TODO`, `FIXME`, `HACK`
   or `XXX`; no `breakpoint`, `pdb`, `ipdb` or `set_trace`; no assignment of a
   secret-shaped literal to a credential-named variable; and three `#` lines
   that begin like code (`# signal = EMA of ...`, `# k = 2/(3+1) = 0.5; ...`,
   `# return 10%, drawdown 25% => 10 / 26`), all three of which are prose or
   arithmetic notes and none of which is a run of two or more code-like lines.
   The rule was right to say nothing about all three, which is the plan B
   correction ("Python prose no longer reads as code", `71fcb9c`) doing its
   job on real files.

What the Python `leftover-debug` does not read, by design, is `print()`:
FastSpot has dozens, in scripts and a dashboard that print on purpose, and the
rule's Python vocabulary is the debugger entry points. That is the plan B
ruling and this corpus is evidence for it: reading `print` as debug would have
produced dozens of findings on a repository where every one is output.

The honest limit: 96 files is a small corpus, and a corpus that holds none of
what four rules look for measures none of them. The Python pairs ship on
fixture evidence plus these three checks, which is the standing plan 3 gave the
Express rules, and a larger Python repository is what they still owe.

## Gate

Applying the brief's rule (at least 17 of 20, or 85 percent of a sample of at
least 5, ships on; below ships off for that language; fewer than 5 findings is
unmeasured and ships on):

- **No pair fails.** No pair has five findings. The four PHP pairs with
  findings score 1/3, 0/3, 2/2 and 0/1, which is 3 of 9 true across all of
  them, and the honest summary of that is that BookStack is a mature codebase
  with almost nothing left over in it, and two of the three false-positive
  classes (return-mode `print_r`, a documented block in a frozen migration) are
  contained fixes named above.
- **Every pair ships on.** `enabled_for` answers `true` for every rule and
  every language. The mechanism exists so the next measurement can turn one
  pair off without touching the rule's other languages.
- **One STOP for the founder**, on the locked rule: the OIDC test fixture key.
  Not a gate question, and recorded as the plan 3 standard requires.

### The mechanism

`Rule::enabled_for(&self, lang: Language) -> bool`, default `true`, consulted by
`run_rules` on every finding beside the existing `enabled` check: a finding
whose file is in a language the rule answers `false` for is dropped, a file
naming no language (a lockfile, a `.sql` migration) is never asked, and a config
`[rules.<id>] enabled = true` does not override a per-language off, because
that key says whether the rule runs at all and the languages a rule failed on
are the engine's measurement rather than the repository's choice. The intended
knob is a `[rules.<id>] languages = [...]` override, which does not exist yet
and is not needed while nothing is off. A rule that fails a pair overrides the
method with a doc comment citing this report. The test
`a_rule_off_for_one_language_keeps_its_findings_elsewhere` pins all three
properties on a rule that declares every language and is off for PHP.

## Run times

Recorded, not benchmarked: a corpus pass reads every file, so its wall depends
on the page cache and on what else the machine is doing, and this box has
drifted by a factor of two and more between sessions in this ledger before.

| Run | Engine | Wall |
| --- | --- | --- |
| BookStack, offline, fresh cache, first read after the clone | 76056 ms | 76734 ms |
| BookStack, online, warm cache (advisory fetch for two lockfiles) | 5780 ms | 6676 ms |
| BookStack, offline, warm cache | 239 ms | 794 ms |
| FastSpot, offline, fresh cache | 578 ms | 1057 ms |
| FastSpot, online, warm cache | 64 ms | 558 ms |
| Python probe (1 file) | 102 ms | |
| Composer probe (1 package, online) | 1214 ms | |
| PyPI probe (1 package, online) | 3647 ms | |

### The 76 second pass, re-measured

The first BookStack pass is the largest wall number this ledger holds for a
repository of its size, so it was re-measured after the verification build had
finished, on a box with no cargo, rustc or locrin process running, twice, each
on a fresh cache, offline:

| Run | Engine | Wall | Findings |
| --- | --- | --- | --- |
| cold 1 | 5783 ms | 6523 ms | 340, identical |
| cold 2 | 5422 ms | 6104 ms | 340, identical |

So the engine's cold pass over 2152 files (1773 of them PHP, parsed with the
full HTML-and-PHP grammar) is about 5.5 seconds, which is 2.5 ms per file
against the 2.1 ms per file plan 3 recorded for fasting-app's 1846 TypeScript
files on a quiet day. The 76 seconds was the first read of a checkout the
operating system had just written, on a box that was also finishing a release
build, and it is the same first-run effect plan 3 recorded at 25.9 seconds for
fasting-app. The PHP grammar is not the cost. Neither number is the benchmark;
the benchmark suite runs on the TypeScript bench repository and is unchanged by
this task.

## What ships

- `enabled_for` on the `Rule` trait, `true` everywhere, consulted by
  `run_rules`, with its test.
- No rule changes. Three contained follow-ups named above: return-mode
  `print_r` and `var_export` are not debug sinks; a `commented_code_allowed`
  glob list defaulting to migrations; ordering `ECOSYSTEM` ranges so Packagist
  and PyPI findings can name an upgrade.
- One founder ruling owed: a private key in a test helper under the locked
  rule.
- Two measurements still owed: a Python repository with a lockfile and enough
  leftovers to score, and, for the PHP pairs, a second PHP repository with
  more than nine findings between four rules.
