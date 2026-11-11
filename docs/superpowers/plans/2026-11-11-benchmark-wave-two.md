# Benchmark Wave Two And Locrin 1.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One fixed-size, signal-targeted corpus pass that gives the unscored rules their public numbers, then act on those numbers once and release Locrin 1.0, which closes the project.

**Architecture:** The benchmark's corpus builder gains a filter that keeps an agent-written commit only when its added lines touch something an unscored rule looks at (a "signal"), with a quota per signal. The new diffs are labelled by the same two-pass protocol as wave one and scored by the unchanged harness. Whatever the numbers say is applied to the engine's defaults in a single pass, and that engine is tagged 1.0.0.

**Tech Stack:** Python 3.12 and pytest in `locrin-benchmark` (harness, no new dependency); `gh` for the GitHub API; Rust workspace in `locrin` for the defaults pass and the release.

**Spec:** There is no separate spec. The founder waived the question round on 2026-11-11 ("go with your recommendation, I want to finish this project") and delegated the calls. The Design section below is the spec, and every decision in it is the plan author's call, not something the founder was asked.

## Design

### The finish line

Locrin is finished when all five of these are true, and nothing else is in scope:

1. Wave two is published (Tasks 1 to 6).
2. Every rule that ships on has either a public score at n of 5 or more, or a stated reason in the public table for why it has none.
3. No rule ships on with a public precision under 85 percent (spec 4.1's bar).
4. The three excluded label disputes are settled.
5. 1.0.0 is tagged, released on the four channels and measured by the nightly.

Not part of finishing, and not to be started under this plan: the hosted layer, licence-key packs, LSP and editor extensions, competitor comparisons (spec 8.3, 8.4, 10.5, 11.4), a third corpus wave, the DoS and DAST idea. They are a separate product decision that needs users first; the registries show none yet.

### Why a signal filter

Measured on wave one's 214 diffs on 2026-11-11, the share whose added lines even touch each rule's subject: weak crypto 2.8 percent, HTML sinks 2.8, test skips 0.9, `cors(` 0.0, `.cookie(` 0.5, service role 0.0, non-test `console` 6.1, markers 0.5, new test cases 19.6. Sampling more commits at random would need thousands of labelled diffs. Filtering candidates by a cheap text signal before they are accepted costs API calls only; labelling cost is paid only for diffs that can move a number.

A signal must be a superset of what its rule can flag: it selects diffs where the rule's subject appears, never diffs where the rule fires. Precision and recall are then measured on the population the rule can ever speak about, which is why wave-two diffs can be pooled with wave one in one score. The README says so.

### The budget (fixed, one pass)

At most 110 new diffs, TypeScript and JavaScript only (half of wave one's files are PHP and Python, where 16 of the 21 rules do not run). Quotas, rarest first, which is also the order a diff matching several signals is assigned in:

| Signal | Feeds rule | Quota |
| --- | --- | --- |
| `supabase-service-role` | `supabase-service-role-in-client` | 9 |
| `cipher-iv` | `weak-crypto` | 6 |
| `express-cookie` | `express-cookie-insecure` | 10 |
| `weak-hash` | `weak-crypto` | 8 |
| `test-skip` | `test-newly-skipped` | 12 |
| `marker` | `leftover-agent-marker` | 12 |
| `weak-random` | `weak-crypto` | 6 |
| `html-sink` | `html-injection` | 12 |
| `console` | `leftover-debug` | 15 |
| `test-case` | `test-no-assert` | 20 |

Three builder runs, each stopped by the operator at three hours if it has not ended. A quota that is not full after the three runs stays short: the rule is reported with what it has. There is no wave three.

### Rules wave two does not target, and the reason the table will give

- `secret-exposed`: a public corpus must not redistribute live credentials.
- `express-cors-wildcard-on-authenticated`: it fires only where a repository's own `locrin.toml` names its auth middleware, and the harness writes one config for every repository, so no diff could move its number. Found while executing Task 1; the rule joins the ones marked not benchmarked.
- `supabase-table-without-rls`: reads SQL migrations, which the corpus does not store (`sql` is in the sparse-checkout skip list).
- `unreachable`: no text signal is cheaper than the rule itself; scored only if the new diffs happen to supply it.
- `unused-import`: 1 reported finding in 71 import-adding diffs; every new diff adds imports, so it accrues without a quota.
- `leftover-commented-code`: ships off.
- `boundary-violation`, `vulnerable-dependency`, `express-route-without-auth`: already marked not benchmarked.

### Rulings made during execution

- The `express-cors` signal and its quota of 10 were dropped, for the reason above. The budget is 110, not 120.
- The `supabase-service-role` signal matches the name of the key and never a pasted key. A diff chosen for holding one would put a live credential in a public corpus. It is the one place a signal is knowingly not a superset, and the README says so.
- Task 1 Step 5 found two real holes, both closed and pinned by tests: `createDecipheriv` under `weak-crypto` and `it.skip.each` under `test-skip`.
- An independent review of Tasks 1 and 2, run before Task 3 because the long builds and the labelling spend rest on that code, found the signals would have spent the budget badly in three places. All fixed test first: the test signals read the files the test rules open (`*.test.*`, `*.spec.*`, `__tests__`) and match a test callee, not a word; `weak-crypto` is three signals, one per shape the rule reads, with `weak-hash` narrowed to `md5` and `sha1`; the service-role signal is spelled as the rule spells it and skips the default server paths; `console` matches an optional-chained call. The order is rarest first as re-measured on wave one with the final patterns, which is the table above. `--signals` searches TypeScript and JavaScript only and, without `--target`, is bounded by its quotas.
- The base rates under "Why a signal filter" were measured with rough patterns over every language. With the final signals on TypeScript and JavaScript files, six of the ten appear in none of wave one's 214 diffs, `weak-random` in 0.9 percent, `console` and `html-sink` in 2.3 percent each and `test-case` in 15.0 percent.
- Task 1's and Task 2's code blocks below are as first written. The signal names, patterns and tests that shipped are the ones in `bench/signals.py` and `tests/test_signals.py` on the benchmark's `main`.
- macOS 14 runners were retired by GitHub after 0.6.1; the engine's CI and release workflows moved to `macos-15` in their own pull request, without which no release could build.

### What happens to a rule that scores badly

A rule that ships on and scores under 85 percent precision at n of 5 or more ships off in 1.0, exactly as `leftover-commented-code` did in 0.6.0, unless its false positives share one cause that a change of under a day removes (the precedent is the test-file exemption of 0.6.1). `html-injection` is the likely case: it stands at 1 true and 3 false.

## Global Constraints

- No em dashes in any file, commit message, pull request or label note.
- Commits carry no AI attribution of any kind and read as the repository owner's.
- Stage by explicit path. Never `git add -A`, `git add .` or `git commit -a`.
- Every change goes branch, pull request, merge. Nothing is pushed to `main` directly. A child pull request is retargeted before its base is merged with `--delete-branch`.
- Test first: the failing test is written and seen to fail before the code.
- Every background run longer than ten minutes gets a Monitor watchdog armed at launch that reports completion or a ten-minute stall. Agents run git as `git --no-pager` and never wait on a hung command.
- Labelling agents need the founder's go first, given against a stated count of diffs and agents (Task 4, Step 1). Nothing else in this plan waits on the founder except Task 7, Step 2.
- The corpus and its labels land in one pull request. A corpus merged without labels makes the nightly exit 4 every night until the labels follow.
- Python 3.12 or newer. On Windows launch detached shell scripts with `"C:\Program Files\Git\bin\bash.exe" -l`, never the `bash` on PATH (that one is WSL).
- Corpus limits are unchanged: licences MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC; 3 records per repository; 6 per owner; 30 files and 200,000 bytes per file.

## Review Focus

1. A commit file with no `patch` key (GitHub omits it for large and binary files): the signal matcher treats it as no added lines, it does not raise.
2. A signal that appears only on a removed or context line: not a match.
3. An added line whose own text starts with `+` (`++i;` arrives as `+++i;`): it is an added line, and is not mistaken for a file header.
4. A diff matching several signals when the rarest one's quota is full: it is assigned to the next matched signal that still has room, and is skipped only when none has.
5. A wave-one record, which has no `wave`, `signal` or `signals` key: it loads as wave 1 with no signals, and a build resumed over a corpus holding both kinds counts only wave-two records toward the quotas.

Each is pinned by a named test in Task 1 or Task 2.

---

### Task 1: The signal matcher

**Files:**
- Create: `bench/signals.py` (in `locrin-benchmark`)
- Test: `tests/test_signals.py`

**Interfaces:**
- Produces:
  - `SIGNALS: dict[str, Signal]`, insertion-ordered rarest first: `weak-crypto`, `express-cookie`, `supabase-service-role`, `test-skip`, `html-sink`, `test-case`, `marker`, `console`.
  - `WAVE_TWO_TARGETS: dict[str, int]`, the quota table above (sums to 110).
  - `added_lines(patch: str | None) -> list[str]`
  - `is_test_file(path: str) -> bool`
  - `matched(files: list[dict]) -> list[str]`, signal names in `SIGNALS` order.
  - `pick(matched: list[str], held: dict[str, int], targets: dict[str, int]) -> str | None`

- [ ] **Step 1: Write the failing tests**

```python
from bench.signals import SIGNALS, WAVE_TWO_TARGETS, added_lines, is_test_file, matched, pick


def f(name, *added, removed=()):
    body = "".join(f"-{l}\n" for l in removed) + "".join(f"+{l}\n" for l in added)
    return {"filename": name, "status": "modified", "patch": "@@ -1,2 +1,3 @@\n context\n" + body}


def test_the_signals_are_ordered_rarest_first_and_the_quotas_sum_to_the_budget():
    assert list(SIGNALS) == ["weak-crypto", "express-cookie", "supabase-service-role",
                             "test-skip", "html-sink", "test-case", "marker", "console"]
    assert set(WAVE_TWO_TARGETS) == set(SIGNALS) and sum(WAVE_TWO_TARGETS.values()) == 110


def test_added_lines_keeps_only_added_lines_without_their_marker():
    patch = "@@ -1,2 +1,3 @@\n context\n-gone();\n+kept();\n+++i;\n\\ No newline at end of file"
    assert added_lines(patch) == ["kept();", "++i;"]


def test_a_file_without_a_patch_has_no_added_lines():
    assert added_lines(None) == []
    assert matched([{"filename": "src/a.ts", "status": "modified"}]) == []


def test_a_signal_on_a_removed_or_context_line_is_not_a_match():
    assert matched([f("src/a.ts", "const x = 1;", removed=["console.log(x);"])]) == []
    assert matched([{"filename": "src/a.ts", "patch": "@@ -1 +1 @@\n console.log(1);\n+const y = 2;\n"}]) == []


def test_each_signal_matches_its_subject():
    cases = {
        "weak-crypto": 'const h = createHash("md5").update(pw).digest("hex");',
        "express-cookie": 'res.cookie("sid", id);',
        "supabase-service-role": "const key = process.env.SUPABASE_SERVICE_ROLE_KEY;",
        "html-sink": "el.innerHTML = body;",
        "marker": "// TODO: handle the empty case",
        "console": 'console.log("here", x);',
    }
    for name, line in cases.items():
        assert matched([f("src/a.ts", line)]) == [name], name
    assert matched([f("src/a.test.ts", 'it.skip("renders", () => {});')]) == ["test-skip"]
    assert matched([f("src/a.test.ts", 'it("renders", () => { run(); });')]) == ["test-case"]


def test_weak_crypto_also_matches_math_random_and_a_bare_sha1():
    assert matched([f("src/a.ts", "const token = Math.random().toString(36);")]) == ["weak-crypto"]
    assert matched([f("src/a.js", 'crypto.createHash("SHA1")')]) == ["weak-crypto"]


def test_console_is_not_a_signal_in_a_test_file_or_an_allowed_path():
    for path in ("src/a.test.ts", "test/run.mjs", "scripts/build.ts", "bin/cli.js", "vite.config.ts"):
        assert "console" not in matched([f(path, "console.log(1);")]), path


def test_a_new_test_case_is_a_signal_only_in_a_test_file():
    assert matched([f("src/a.ts", 'test("x", () => {});')]) == []


def test_only_the_javascript_family_is_read():
    for path in ("app/a.php", "app/a.py", "README.md", "db/001.sql"):
        assert matched([f(path, "// TODO: later", "console.log(1);")]) == [], path
    for path in ("a.ts", "a.tsx", "a.js", "a.jsx", "a.mjs", "a.cjs"):
        assert matched([f(path, "// TODO: later")]) == ["marker"], path


def test_several_signals_come_back_in_signal_order():
    got = matched([f("src/a.ts", "console.log(1);", "// FIXME"), f("src/b.ts", 'res.cookie("sid", id);')])
    assert got == ["express-cookie", "marker", "console"]


def test_is_test_file_is_the_engine_definition():
    for path in ("test/a.mjs", "src/tests/a.ts", "__tests__/a.js", "spec/a.ts", "src/a.test.ts", "src/a.spec.tsx",
                 "src/scope-prop-test.mjs", "src/bar_test.js"):
        assert is_test_file(path), path
    for path in ("src/latest.ts", "src/contest.ts", "src/testing/util.ts"):
        assert not is_test_file(path), path


def test_pick_takes_the_rarest_matched_signal_with_room():
    targets = {"weak-crypto": 1, "marker": 2, "console": 2}
    assert pick(["weak-crypto", "console"], {}, targets) == "weak-crypto"
    assert pick(["weak-crypto", "console"], {"weak-crypto": 1}, targets) == "console"
    assert pick(["weak-crypto", "console"], {"weak-crypto": 1, "console": 2}, targets) is None
    assert pick(["html-sink"], {}, targets) is None
    assert pick([], {}, targets) is None
```

- [ ] **Step 2: Run them and see them fail**

Run: `python -m pytest -q tests/test_signals.py`
Expected: an import error for `bench.signals`.

- [ ] **Step 3: Implement `bench/signals.py`**

`Signal` is a frozen dataclass of `name`, a compiled `pattern` and a `wants(path) -> bool` file filter. `matched` reads only files whose extension is one of `.ts .tsx .js .jsx .mjs .cjs`, and a signal matches when its pattern finds any added line of a file its filter accepts. The patterns and filters, which are the decisions:

| Signal | Pattern on an added line | File filter |
| --- | --- | --- |
| `weak-crypto` | `createHash\(`, `createHmac\(`, `create(De)?cipher(iv)?\(`, `Math\.random\(`, or the word `md5` or `sha1` in any case | any |
| `express-cookie` | `\.cookie\(` | any |
| `supabase-service-role` | `service_role` or `serviceRole`, any case | any |
| `test-skip` | `\.(skip|todo)\b`, `\bxit\(`, `\bxtest\(`, `\bxdescribe\(` | any |
| `html-sink` | `dangerouslySetInnerHTML`, `\.innerHTML\b`, `\.outerHTML\b`, `insertAdjacentHTML`, `document\.write\(`, `\.html\(` | any |
| `test-case` | `\b(it|test)\(` | `is_test_file` |
| `marker` | `\b(TODO|FIXME|HACK|XXX)\b` | any |
| `console` | `\bconsole\.(log|debug|trace|dir|table)\(` or `\bdebugger\b` | not `is_test_file`, and not under a `scripts` or `bin` directory, and the name does not contain `.config.` |

`is_test_file` is the definition in the engine's `crates/rules/src/leftover_debug.rs` (`TEST_DIRS` and `TEST_SUFFIXES`), copied by value with a comment naming that file.

- [ ] **Step 4: Run the tests and see them pass**

Run: `python -m pytest -q tests/test_signals.py`
Expected: all pass.

- [ ] **Step 5: Prove each signal is a superset of its rule, against the engine's own fixtures**

This is a one-off check, not a committed test, because the fixtures live in the other repository. For each rule in the quota table, run the engine (`locrin check --json`) over that rule's `flag` fixture directory under `crates/rules/tests/fixtures/` in the engine repository, and for every finding of that rule assert that `SIGNALS[<signal>].pattern` matches the finding's line or a line of the same statement in that file. Expected: zero unmatched findings. A finding the signal cannot see is a hole in the superset: widen the pattern, add a case for it to `test_each_signal_matches_its_subject`, and run again. Put the per-rule counts (findings, matched) in the pull request body.

- [ ] **Step 6: Commit**

```bash
git add bench/signals.py tests/test_signals.py
git commit -m "bench: signals, the text filters that pick wave two's diffs"
```

### Task 2: The builder keeps a commit only when a signal has room

**Files:**
- Modify: `bench/build_corpus.py` (`repo_query`, `repositories`, `accept`, `write_record`, the `--via-repos` loop, `main`)
- Modify: `bench/corpus.py` (`Diff`, the record loader)
- Modify: `README.md` ("What is in the corpus"), `CONTRIBUTING.md` ("Building many diffs at once")
- Test: `tests/test_build_corpus.py`, `tests/test_corpus.py`

**Interfaces:**
- Consumes: `matched`, `pick`, `SIGNALS`, `WAVE_TWO_TARGETS` from Task 1.
- Produces:
  - `repo_query(language: str, licence: str, since: str, topic: str | None = None) -> str`, which appends ` topic:<topic>` when given; `repositories(...)` takes and passes the same `topic`.
  - `accept(..., signal_pick=None)`, where `signal_pick(files: list[dict]) -> tuple[str, list[str]] | str` returns `(assigned, matched)` or a skip reason. It is called once, straight after `language_skip`, before any file is fetched. With a tuple the record gains `"wave": 2`, `"signal": assigned`, `"signals": matched`.
  - `Diff.wave: int` (default 1), `Diff.signal: str | None`, `Diff.signals: list[str]`.
  - CLI, with `--via-repos` only: `--signals` (use the filter with `WAVE_TWO_TARGETS`), `--signal-target NAME=K` (repeatable, overrides one quota, `K=0` turns a signal off), `--topic T` (repeatable; one repository search per topic, language and licence).

- [ ] **Step 1: Write the failing tests**

In `tests/test_build_corpus.py`, using the file's existing fake `gh`:

- `test_repo_query_names_a_topic_when_given`: `repo_query("typescript", "mit", "2026-09-01", topic="supabase")` ends with ` topic:supabase`; without `topic` the string is byte-identical to today's.
- `test_a_commit_no_signal_matches_is_skipped_before_any_file_is_fetched`: `accept` with a `signal_pick` returning `"no signal"` returns `None`, `SKIPS["no signal"] == 1`, and the fake recorded no `contents` call.
- `test_an_accepted_wave_two_record_names_its_signal`: `signal_pick` returning `("marker", ["marker", "console"])` gives a record with `wave == 2`, `signal == "marker"`, `signals == ["marker", "console"]`.
- `test_accept_without_signal_pick_writes_no_wave_keys`: the record has none of `wave`, `signal`, `signals`, so wave-one builds stay byte-identical.
- `test_a_full_quota_falls_through_to_the_next_matched_signal`: a `--via-repos --signals --signal-target weak-crypto=1` build over two fake commits that both match `weak-crypto` and `console` writes one record with `signal == "weak-crypto"` and one with `signal == "console"`.
- `test_a_commit_whose_every_matched_signal_is_full_is_skipped_as_signal_target`: `SKIPS["signal target"] == 1`.
- `test_a_resumed_build_counts_wave_two_records_already_written_and_ignores_wave_one`: an `--out` holding one wave-one record and one wave-two record with `signal == "marker"`, built with `--signal-target marker=1`, accepts no further `marker` diff.
- `test_signals_without_via_repos_is_refused` and `test_an_unknown_signal_target_is_refused`: exit code 2 and a message naming the option.

In `tests/test_corpus.py`:

- `test_a_record_without_wave_keys_loads_as_wave_one`: `wave == 1`, `signal is None`, `signals == []`.
- `test_a_wave_two_record_loads_its_signal_and_refuses_one_that_is_not_a_signal`: a record with `"signal": "nonsense"` raises `CorpusError`; so does `"wave": 2` with no `signal`.

- [ ] **Step 2: Run them and see them fail**

Run: `python -m pytest -q tests/test_build_corpus.py tests/test_corpus.py`
Expected: the new tests fail, the existing ones pass.

- [ ] **Step 3: Implement**

The `--via-repos` loop builds `held` from the `signal` of every wave-two record already in `--out`, and its `signal_pick` is `matched(files)` then `pick(...)`: `"no signal"` when nothing matched, `"signal target"` when nothing matched has room, else the tuple, incrementing `held` only once the record is written. The loop also ends when every quota is full. The summary line gains the records written per signal. With `--topic`, the repository search runs once per topic in place of the plain query.

- [ ] **Step 4: Run the whole suite**

Run: `python -m pytest -q`
Expected: all pass (590 passed before this task, plus the new ones).

- [ ] **Step 5: Document the selection**

README, "What is in the corpus": add one paragraph saying that wave two's diffs (records with `"wave": 2`) were kept only when their added lines matched a named signal, listing the nine signals and quotas, stating that a signal is a superset of what its rule can flag so the selection picks where a rule's subject appears and not where the rule fires, and that this is why both waves are pooled in one score. CONTRIBUTING, "Building many diffs at once": document `--signals`, `--signal-target` and `--topic` in the existing bullet style.

- [ ] **Step 6: Commit and open the pull request**

```bash
git add bench/build_corpus.py bench/corpus.py tests/test_build_corpus.py tests/test_corpus.py README.md CONTRIBUTING.md
git commit -m "bench: build_corpus keeps a commit only when a signal has room"
```

Tasks 1 and 2 are one pull request, `bench/signals`, merged on green. Merging it changes the harness digest, so the next nightly measures v0.6.1 again over the unchanged corpus: expected, and the table must come out identical.

### Task 3: Build wave two

**Files:**
- Create: about 120 records under `corpus/` (in `locrin-benchmark`, on a new branch `bench/wave-two`, not merged until Task 5)

- [ ] **Step 1: Run the three builds, rare stacks first**

Each run is detached with its stderr in a log and a watchdog armed, and is stopped at three hours if it has not ended. A stopped run keeps what it wrote.

```bash
python -m bench.build_corpus --via-repos --since 2026-09-01 --out corpus --language typescript --language javascript --signals --topic express --topic expressjs
python -m bench.build_corpus --via-repos --since 2026-09-01 --out corpus --language typescript --language javascript --signals --topic supabase
python -m bench.build_corpus --via-repos --since 2026-09-01 --out corpus --language typescript --language javascript --signals
```

No `--target`: under `--signals` a build stops at what the corpus holds plus every signal's quota. The first run turns every signal off but `express-cookie` and the second every one but `supabase-service-role` (`--signal-target NAME=0`), so a rare stack's repositories are searched for that stack's subject only and the third run fills the rest from repositories at large.

- [ ] **Step 2: Check what was built**

Run: `python -m bench.build_corpus --check-gone --out corpus` (expected: no `gone:` line) and `python -m pytest -q` (expected: all pass). Tabulate records per signal against the quota table from the last run's summary line.

- [ ] **Step 3: Commit the corpus on the branch**

Stage `corpus` by name and commit as `corpus: wave two, <N> diffs chosen by signal`, with the per-signal counts and any short quota in the body. Push the branch. Do not open the pull request yet.

### Task 4: Label wave two

**Files:**
- Create: one `labels/<id>.json` per wave-two diff
- Modify: `LABELLING.md` ("Two passes")

- [ ] **Step 1: Founder go**

Report the number of diffs built, the number of agents (five diffs per agent, per pass) and the two passes, and wait for the go. This is the only spend in the plan.

- [ ] **Step 2: Add the evidence rule to the protocol first**

LABELLING.md, "Two passes", one paragraph: a note that asserts a property of the code (a shebang, an import, a caller, a config entry) must quote the line it read or name the file and line. The reason goes with it: both wave-one passes described `test/scope-prop-test.mjs` as a `#!/usr/bin/env node` suite, and the file has no shebang. Commit as `labelling: a note quotes the line it relies on`.

- [ ] **Step 3: Write the templates**

For each wave-two id, in id order: `python label.py new <id> --locrin v0.6.1 --by opus`, as a detached loop in the shape of the 0.6.1 carry loop, with `locrin 0.6.1` on PATH. Then copy each unfilled file to `.work/pass2-templates/<id>.json` before any verdict is written, so pass two cannot see pass one.

- [ ] **Step 4: Pass one, by `opus`**

Agents on the Opus model, five diffs each, each given LABELLING.md in full, the diff's before and after files and its checkout. Each sets `pass1` on every entry and adds `missed` entries, then reports the ids it finished.

- [ ] **Step 5: Pass two, blind, by `fable`**

Agents on the Fable model, five diffs each, working only from the templates of Step 3, each writing `.work/pass2/<id>.json` in the format LABELLING.md gives.

- [ ] **Step 6: Merge, settle one-sided misses, confirm**

`python label.py merge <id>` for every id. Where one pass wrote a `missed` entry the other did not, the other pass's model looks at that construct and records its own verdict, as the protocol says; nothing is copied across. Then `python label.py confirm <id> --by fable` for every id.

- [ ] **Step 7: Verify and commit**

Run: `python label.py status --labels labels`. Expected: zero unfilled, every wave-two file confirmed, and a count of new disagreements, which stay excluded. Stage `labels` and `LABELLING.md` by name and commit as `labels: wave two, two passes`.

### Task 5: Publish

- [ ] **Step 1: Open, check and merge the pull request**

`bench/wave-two` into `main`, holding Task 3's and Task 4's commits. Body: diffs per signal against quota, label entries, agreement between the passes, disputes. Merge on green.

- [ ] **Step 2: Measure**

`gh workflow run benchmark.yml -f version=v0.6.1 -f commit=true`, watchdog armed. Wave one's 214 diffs took 20 to 37 minutes on the runner; the job limit is 120. Expected: success and a `results: locrin v0.6.1` commit on `main`. Exit 3 means a source vanished between build and run: prune it by CONTRIBUTING and dispatch again.

- [ ] **Step 3: Read the table**

Record for every rule: true, false positive, missed, and whether it now reaches n of 5. This is the input to Task 7.

### Task 6: Settle the three wave-one disputes

- [ ] **Step 1: Put each dispute to the founder with a proposed verdict**

The two Cassette `swallowed-error` catch-with-fallback entries and the host-bridge template `dead-export` entry: for each, both passes' notes, the lines in question quoted, and a recommended verdict with its reason.

- [ ] **Step 2: Record the settlements**

As LABELLING.md's Disputes section requires: both passes edited, with a note that says why and does not start `adjudicated (`. One pull request, `labels: settle the wave-one disputes`. The nightly measures again on its own.

### Task 7: Apply the numbers to the engine, once

**Files:** in `locrin`: the `enabled_by_default` of each affected rule, `crates/rules/src/lib.rs` (`the_measured_defaults_are_what_the_benchmark_scored`), `README.md` (rule table), `docs/RELEASE-NOTES.md`; in `locrin-benchmark`: `bench/run.py` (`BENCH_TOML`) for any rule newly shipping off.

- [ ] **Step 1: Classify every rule that ships on**

From Task 5's table: at or above 85 percent at n of 5 or more (keeps its default); under 85 percent at n of 5 or more (ships off, or is fixed if its false positives share one cause that under a day's work removes); under n of 5 (keeps its default, and the table's note gives the reason from the Design section).

- [ ] **Step 2: One message to the founder**

The classification, and for each rule under the bar the recommendation (off, or the named fix) with the false positives that justify it. Wait for the answer. This and Task 4, Step 1 are the only two founder gates.

- [ ] **Step 3: Implement what was decided, test first**

One pull request per rule. A default change updates the pinning test in `lib.rs` first and sees it fail. A rule newly off is enabled by name in the benchmark's `BENCH_TOML`, with the test that lists the ships-off rules extended, so the harness keeps measuring it.

### Task 8: Locrin 1.0.0

- [ ] **Step 1: Release**

Version pins from 0.6.1 to 1.0.0 in the same sixteen files as the 0.6.1 bump (commit `a6950e6` is the template), leaving the README's historical sentences about 0.6.0 as they are. Release notes cut as `## 1.0.0`, opening with what 1.0 means: every rule that ships on has a public score or a stated reason for none. Pull request, merge, annotated tag `v1.0.0`, and verify npm, PyPI, crates.io and the Homebrew tap.

- [ ] **Step 2: Carry and measure**

`cargo install --path crates/cli --locked --force`, then `python label.py carry <id> --locrin v1.0.0` for every diff, judge with two passes anything it leaves to judge, one pull request, then dispatch the benchmark for `v1.0.0`.

- [ ] **Step 3: Close out**

Delete the merged remote branches on `locrin` (`engine/commented-prose`, `engine/debug-exemptions`, `engine/debug-test-files`, `engine/defaults-0.6`, `release/0.6.1`). Merge `plan/public-benchmark` (the wave-one plan, one file) so the plans directory is complete. Hand the founder the FastLift step: pin `BilalEjaz/locrin/action@v1.0.0`, re-create the baseline, triage the `vulnerable-dependency` findings. Record the project as closed in memory, with the out-of-scope list from the Design section as the only things that would reopen it.
