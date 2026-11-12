# Public Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A public repository, `BilalEjaz/locrin-benchmark`, whose one command installs a named Locrin version, runs it over a labelled corpus of agent-written diffs from open-source projects, and prints precision and recall per rule and per rule-language pair, with the same numbers on any machine.

**Architecture:** A Python 3.12 package `bench/` with four seams: `corpus` (records and loading), `materialise` (turn a record into a git checkout with a base ref), `run` (install Locrin, run `locrin check --base` in SARIF mode, normalise findings), `score` (join findings with confirmed labels, print the table). A `run.sh` wrapper strings them together. The fixture corpus of ten synthetic diffs ships in the repo and doubles as the end-to-end test; the real corpus is built from GitHub commit search by `build_corpus.py` and labelled in two passes by `label.py`. CI runs the tests on every push and a scheduled job regenerates results for any Locrin release that has none.

**Tech Stack:** Python 3.12 standard library plus `pytest`; bash; git; Locrin installed by the public `install.sh`; GitHub Actions on `ubuntu-latest`.

**Spec:** `docs/superpowers/specs/2026-09-12-public-launch-design.md`, section 5, in the `BilalEjaz/locrin` repository. This plan is stored there too; the benchmark repository is a separate public repository and this plan builds it from empty.

## Global Constraints

- Repository: `github.com/BilalEjaz/locrin-benchmark`, public, MIT, copyright Raxbi Ltd 2026. Local checkout in a `locrin-benchmark` directory beside the engine checkout. The lead creates the empty public repository before Task 1; every task works in that checkout.
- Never write into any other repository on this machine. The Locrin engine repository (`code-quality-platform`) is read-only for this plan except for the final task, which links the results.
- No em dashes anywhere: not in code, comments, docs, commit messages or the results table. Use a comma, a colon or a full stop.
- Commits carry no `Co-Authored-By` trailer and no AI attribution of any kind.
- Stage by explicit path. Never `git add -A` or `git commit -a`.
- Python 3.12, standard library only in `bench/`; `pytest` is the single development dependency. No `requests`, no `PyYAML`: JSON everywhere.
- Corpus licences: only `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC` (SPDX ids as GitHub reports them). Every record carries the licence id and the source URL. No private repository is ever sampled.
- Locrin interface: `locrin check --base <ref> --sarif --offline` is the only engine call that produces scored findings. SARIF field names are the contract: `results[].ruleId`, `results[].locations[0].physicalLocation.artifactLocation.uri`, `results[].locations[0].physicalLocation.region.startLine`, `results[].partialFingerprints["locrin/id"]`, `results[].properties.confidence`, and `tool.driver.rules[]` with `id`, `defaultConfiguration.enabled`, `properties.languages`.
- Locrin exit codes: 0 pass, 1 block, 2 engine error. The harness treats 0 and 1 as success and 2 as a harness failure that names the diff.
- Every run uses its own `LOCRIN_CACHE_DIR` under the work directory so runs never share incremental state.
- Scoring rules: precision = true / (true + false-positive); recall = true / (true + missed). A rule or pair with fewer than 5 confirmed labelled findings is shown with its count and marked `n<5, not scored`, matching the engine's own gate. The line is 85 percent precision.
- Not benchmarked, and shown as such in the table with the reason: `vulnerable-dependency` (advisory feed changes daily), `boundary-violation` and `express-route-without-auth` (silent until configured per repository).
- The harness never runs or compares another vendor's tool.
- Determinism: same Locrin version, same corpus, same labels, same numbers. Anything that would vary by machine (advisory feeds, clock, network) is excluded or pinned.

---

## Design rulings made by this plan

These refine spec section 5 where it left room. The lead has approved them in conversation on 2026-09-13.

1. **SARIF, not `--json`.** Spec 5.2 names `--json` as the interface, but `--json` is the agent form capped at ten findings. `--sarif` carries every finding with the same ids, so the harness reads SARIF.
2. **Full checkout per diff.** Spec 5.1 stores the changed files before and after. The graph rules (`dead-file`, `dead-export`, `unused-import`, `unreachable`) need the whole repository, so a diff is scored inside a partial clone of the source repository checked out at the commit, with `--base <parent>`. The before and after files are still stored in the corpus as the human-readable, licence-carrying record of what was labelled. The fixture corpus stores whole tiny repositories, so it runs with no network.
3. **`vulnerable-dependency` is not scored.** Its findings depend on the advisory feed and would change day to day. The engine's own fixtures test it. The table shows it as `not benchmarked: advisory feed`.
4. **Ships-off rules run.** The benchmark config enables `dead-file`, `swallowed-error` and `injection-sink` and widens `leftover-commented-code` to Python, so every measurable rule and pair gets a number and the table shows the shipped default beside it. The two rules that need per-repository configuration stay unconfigured and are marked.
5. **Results regenerate on a schedule, not on a cross-repository trigger.** A daily job in the benchmark repository reads Locrin's latest release tag; if `results/<version>/` is missing it runs the harness and commits the results and the README table with the workflow's own token. No new secret is created anywhere.
6. **Labelling is a two-model pass.** Pass one by an Opus implementer, pass two by a Fable reviewer, per the founder's model rotation; a label counts only when both passes agree. Disagreements are listed for the lead and excluded until resolved.

---

## Execution amendments (2026-09-13)

Found while preparing execution. Each amendment overrides the task text it names; where a code block below disagrees with an amendment, the amendment wins.

- **A1, Task 1.** Also create `corpus/README.md` and `labels/README.md`, one paragraph each saying what the directory holds, so both directories exist on a fresh clone and `load_corpus("corpus")` works on an empty corpus.
- **A2, Tasks 2, 5 and 7.** Fixture labels must be truthful under LABELLING.md. There is no deliberately wrong label. Instead the fixtures carry at least one genuine false positive and at least one genuine missed entry. `fx-01-debug` also adds `src/logger.ts`, a module whose exported `info(msg: string)` calls `console.log` as that module's real logging: 0.5.0 is expected to report `leftover-debug` there, and the truthful verdict is `false-positive`. Drop the `// TODO(agent): remove` line from `fx-01-debug`. `fx-03-unreachable` carries a construct that meets the `unreachable` definition and that 0.5.0 does not report, verified by running the engine; if no such construct exists for `unreachable`, pick the rule where a genuine miss does exist and record the choice. Task 5's report states the exact per-rule and per-pair numbers the truthful labels produce, and Task 7's end-to-end test asserts those numbers, including at least one row with precision below 100 percent and one with recall below 100 percent.
- **A3, Task 2.** `fx-05-secret` must not trip GitHub push protection. Avoid every GitHub secret-scanning partner shape (AWS key id with a secret, Stripe live keys, GitHub, Slack, npm and PyPI tokens, private key blocks). Use a shape 0.5.0 flags that is not a partner pattern, such as a high-entropy literal in a credential-named assignment, or a JWT whose payload names `service_role`, and verify it fires. The file carries the comment `synthetic fixture, not a real credential`.
- **A4, Task 3.** `_materialise_git` runs `git checkout --detach -f <sha>` and then `git clean -fdq`, so a previous diff's `locrin.toml` or a stripped tracked file never blocks the checkout.
- **A5, Tasks 4, 5 and 7.** `run_check(locrin: Path, checkout: Checkout, work: Path, diff_id: str) -> dict` keys the cache on the diff: `LOCRIN_CACHE_DIR = work/cache/<diff_id>`, because several git diffs share one checkout root. The Task 4 test asserts that exact path. Every caller passes the diff id.
- **A6, Task 4.** Tests pass on Windows and Linux. A fake binary is a `.cmd` file on Windows (`@echo locrin 0.5.0`) and an executable `sh` script elsewhere. `install_locrin` without `LOCRIN_BIN` on Windows uses `shutil.which("locrin")` when its version matches and otherwise raises `RunError` telling the user to set `LOCRIN_BIN`, because `install.sh` refuses to run on Windows.
- **A7, Tasks 8, 9 and 10.** The corpus builder never reads a token. `GitHub.get` runs `gh api --method GET <path> -H "Accept: <accept>" -f key=value ...` and parses stdout. When stderr mentions a rate limit it sleeps 60 seconds once, retries, and then raises `BuildError`. Workflows give `gh` its token through `GH_TOKEN: ${{ github.token }}`. Task 10 runs `python -m bench.build_corpus --out corpus --target 300` with no token on the command line.
- **A8, Task 9.** `benchmark.yml` gains a boolean `commit` input, default true. The commit step runs only on the schedule or when `commit` is true. Task 9 step 3 dispatches with `commit=false` and needs no revert.
- **A9, all tasks.** The benchmark repository is created private and stays private until the founder says "make it public". Nothing is pushed to a branch named `main` in either repository and no pull request is merged. Tasks 1 to 9 live on `bench/harness`, Task 10 on `bench/corpus-wave-one` branched from it, Task 11 on `bench/labels-wave-one` branched from that.
- **A10, Task 11.** Pass two runs as an independent, blind Opus pass recorded as `pass2.by = "opus-blind"`, so a Fable pass can replace it at review. Templates for every diff are created up front by one sequential process and copied to `.work/templates/`. Labelling agents read code with `git show <sha>` and `git show <sha>:<path>` in `.cache/repos/<owner>__<name>`, never check out, and so run in parallel safely. Pass-two agents read only `.work/templates/<id>.json` and write `.work/pass2/<id>.json`; a merge step folds them into `labels/`. Disagreements are adjudicated by an Opus agent whose note starts `adjudicated (opus), pending Fable review`.
- **A11, Task 6.** `language_of` is imported at the top of `bench/score.py`.
- **A12, all tasks.** Commit messages carry no trailer of any kind.
- **A13, Tasks 2 and 5.** Each fixture's `package.json` names that fixture's source files as entry points (an `exports` map), so the dead-code rules (`dead-file`, `dead-export`) stay silent unless a fixture targets them. Any other finding 0.5.0 still reports on a fixture gets a truthful verdict under LABELLING.md. `not-applicable` is used only as its definition says (a file the diff did not really change), never as a way to hide an inconvenient finding.
- **A15, Tasks 4 to 7 and 11 (lead ruling, 2026-09-13).** The unit of measurement is a finding the change introduced. After the run at the commit, the harness runs locrin at the parent with the same config and a fresh cache, over the files that carry findings at the commit and still exist at the parent. A finding whose (rule, file, id) the parent run also reports is pre-existing: it is dropped from that diff's scoring, never gets a label entry, and is counted per rule, per pair, in the table heading and in run.json as pre-existing. Across the corpus a (repository, rule, file, id) is counted by occurrence rank: a diff whose parent held p occurrences of it and which adds n more introduces occurrences p+1 to p+n, and a later diff from the same repository that introduces a rank an earlier diff (by id) already counted is a duplicate, counted per rule, per pair, in the table heading and in run.json as such. A diff that adds another occurrence beside ones its parent already held is not a duplicate, whatever the id order, so a linear history is counted the same way however its ids sort. Missed entries name only constructs the diff introduced. Label templates list only introduced findings. LABELLING.md and README "How it scores" say this in plain words. A rename changes the file path and therefore the id, so a renamed file's findings count as introduced; the docs say so.
- **A14, Task 3.** Throwaway repositories the harness creates set `core.autocrlf=false` (pass `-c core.autocrlf=false` or write it to the repo config right after `git init`), so line endings and line numbers are identical on Windows and Linux.

---

## File structure

```
locrin-benchmark/
  LICENSE                      MIT, Raxbi Ltd 2026
  README.md                    what it is, how to run, results table between markers
  LABELLING.md                 the labelling protocol and per-rule truth definitions
  CONTRIBUTING.md              how to dispute a label, how to add a diff
  run.sh                       one command: install, run, score, write results
  bench/__init__.py
  bench/corpus.py              Diff record dataclass, load/validate corpus and fixtures
  bench/materialise.py         record -> (checkout root, base ref); tree and git sources
  bench/run.py                 install locrin, write locrin.toml, run check, normalise SARIF
  bench/score.py               join findings and labels, compute table, render markdown
  bench/labels.py              label record schema, load/validate, confirmation logic
  bench/label_tool.py          `label.py` CLI: new, confirm, status
  bench/build_corpus.py        GitHub search -> candidate records with before/after files
  bench/readme_table.py        replace the table between README markers
  label.py                     thin entry point for bench.label_tool
  corpus/                      real diffs: <id>.json plus <id>/before/... and <id>/after/...
  labels/                      <id>.json, one per labelled diff
  fixtures/corpus/             ten synthetic diffs, whole tiny repos: <id>.json, <id>/before, <id>/after
  fixtures/labels/             their labels, all confirmed
  fixtures/sarif/              a captured SARIF document for the normaliser test
  fixtures/github/             captured API responses for the corpus builder test
  results/<version>/findings.jsonl, table.md, run.json
  tests/test_corpus.py
  tests/test_materialise.py
  tests/test_run.py
  tests/test_labels.py
  tests/test_score.py
  tests/test_readme_table.py
  tests/test_build_corpus.py
  tests/test_end_to_end.py     runs run.sh on the fixture corpus with a real locrin
  .github/workflows/ci.yml
  .github/workflows/benchmark.yml
```

Diff ids: `<owner>__<repo>__<7 char sha>` for real diffs, `fx-<two digits>-<slug>` for fixtures. Ids are file-system safe on Windows and Linux.

---

### Task 1: Repository scaffold and corpus records

**Files:**
- Create: `LICENSE`, `README.md`, `CONTRIBUTING.md`, `.gitignore`, `pytest.ini`, `bench/__init__.py`, `bench/corpus.py`
- Test: `tests/test_corpus.py`

**Interfaces:**
- Produces: `bench.corpus.Diff` dataclass with fields `id: str`, `source: str` ("tree" or "git"), `repo: str | None` (owner/name), `sha: str | None`, `parent: str | None`, `licence: str`, `language: str` ("typescript", "javascript", "php", "python"), `url: str`, `files: list[str]` (paths changed, forward slashes); `load_corpus(root: Path) -> list[Diff]` sorted by id; `CorpusError(Exception)`; `LANGUAGE_OF_EXT: dict[str, str]` mapping `.ts .tsx .js .jsx .mjs .cjs .php .phtml .py` to engine language names `typescript tsx javascript php python`; `language_of(path: str) -> str | None`.

- [ ] **Step 1: Create the repository files**

`LICENSE`: the MIT text with `Copyright (c) 2026 Raxbi Ltd`.

`.gitignore`:
```
.cache/
.work/
__pycache__/
.pytest_cache/
```

`pytest.ini`:
```ini
[pytest]
testpaths = tests
```

`README.md` (the table section is filled by Task 7's script; keep the markers exactly):
```markdown
# locrin-benchmark

Precision and recall of every [Locrin](https://github.com/BilalEjaz/locrin) rule, measured on a public corpus of agent-written diffs from open-source projects, reproducible on any machine.

## Run it

    ./run.sh v0.5.0

That installs the named Locrin version into `.cache/bin/`, checks out every corpus diff, runs `locrin check --base` on each, and writes `results/v0.5.0/`. It needs bash, git, curl and Python 3.12. The first run clones the source repositories into `.cache/repos/`; later runs are offline.

## Results

<!-- results:start -->
No results yet.
<!-- results:end -->

## How it scores

A finding is matched to a label by rule id, file and start line. Precision is true over true plus false positive; recall is true over true plus missed. A label counts only when two independent passes agree (see LABELLING.md). A rule or rule-language pair with fewer than five confirmed labelled findings is shown with its count and not scored. The line is 85 percent precision, the same gate the engine holds itself to on its private corpus.

Three things are deliberately not scored and say so in the table: `vulnerable-dependency`, because its findings depend on an advisory feed that changes daily; `boundary-violation` and `express-route-without-auth`, because they are silent until a repository configures them.

## What is in the corpus

Diffs from public repositories under MIT, Apache-2.0, BSD or ISC licences, chosen from commits that carry an agent co-author trailer. Each record in `corpus/` names the repository, commit, parent, licence and language, and stores the changed source files before and after. The harness scores each diff inside a checkout of the whole repository at that commit, because the graph rules need it. No private code is ever sampled.

## Licence

MIT. Corpus files keep the licence of the repository they came from, named in each record.
```

`CONTRIBUTING.md`:
```markdown
# Contributing

## Disputing a label

Open an issue titled `label: <diff id> <rule> <file>:<line>` saying which verdict you think is wrong and why, quoting the code. A maintainer re-runs both passes; if they now disagree the label is excluded until resolved.

## Adding a diff

Run `python -m bench.build_corpus --repo owner/name --sha <commit>`; it refuses licences outside MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause and ISC. Then label it with `python label.py new <id>` and get a second pass with `python label.py confirm <id>`.

## Rules for this repository

No em dashes in any text. Commits carry no AI attribution trailer. Stage by explicit path.
```

`bench/__init__.py` is empty.

- [ ] **Step 2: Write the failing test**

`tests/test_corpus.py`:
```python
import json
from pathlib import Path

import pytest

from bench.corpus import CorpusError, Diff, language_of, load_corpus


def write(root: Path, rec: dict) -> None:
    (root / f"{rec['id']}.json").write_text(json.dumps(rec), encoding="utf-8")


def good(**over) -> dict:
    rec = {
        "id": "acme__widgets__abc1234",
        "source": "git",
        "repo": "acme/widgets",
        "sha": "abc1234abc1234abc1234abc1234abc1234abc12",
        "parent": "def5678def5678def5678def5678def5678def56",
        "licence": "MIT",
        "language": "typescript",
        "url": "https://github.com/acme/widgets/commit/abc1234",
        "files": ["src/a.ts", "src/b.tsx"],
    }
    rec.update(over)
    return rec


def test_loads_records_sorted_by_id(tmp_path):
    write(tmp_path, good(id="b__x__1111111"))
    write(tmp_path, good(id="a__x__2222222"))
    diffs = load_corpus(tmp_path)
    assert [d.id for d in diffs] == ["a__x__2222222", "b__x__1111111"]
    assert isinstance(diffs[0], Diff)
    assert diffs[0].files == ["src/a.ts", "src/b.tsx"]


def test_rejects_unknown_licence(tmp_path):
    write(tmp_path, good(licence="GPL-3.0"))
    with pytest.raises(CorpusError, match="licence"):
        load_corpus(tmp_path)


def test_rejects_id_mismatch_and_unknown_language(tmp_path):
    write(tmp_path, good(id="other"))
    with pytest.raises(CorpusError, match="id"):
        load_corpus(tmp_path)
    (tmp_path / "other.json").unlink()
    write(tmp_path, good(language="rust"))
    with pytest.raises(CorpusError, match="language"):
        load_corpus(tmp_path)


def test_tree_source_needs_no_repo_fields(tmp_path):
    write(tmp_path, good(id="fx-01-debug", source="tree", repo=None, sha=None, parent=None, url="fixture"))
    (tmp_path / "fx-01-debug" / "before").mkdir(parents=True)
    (tmp_path / "fx-01-debug" / "after").mkdir(parents=True)
    assert load_corpus(tmp_path)[0].source == "tree"


def test_tree_source_requires_before_and_after_dirs(tmp_path):
    write(tmp_path, good(id="fx-01-debug", source="tree", repo=None, sha=None, parent=None, url="fixture"))
    with pytest.raises(CorpusError, match="before"):
        load_corpus(tmp_path)


def test_language_of_extension():
    assert language_of("src/a.ts") == "typescript"
    assert language_of("src/a.tsx") == "tsx"
    assert language_of("lib/x.mjs") == "javascript"
    assert language_of("app/x.phtml") == "php"
    assert language_of("pkg/m.py") == "python"
    assert language_of("README.md") is None
```

- [ ] **Step 3: Run it to verify it fails**

Run: `python -m pytest tests/test_corpus.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.corpus'`.

- [ ] **Step 4: Write the implementation**

`bench/corpus.py`:
```python
"""Corpus records: one JSON file per diff, validated on load."""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

ALLOWED_LICENCES = {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC"}
LANGUAGES = {"typescript", "javascript", "php", "python"}
LANGUAGE_OF_EXT = {
    ".ts": "typescript",
    ".tsx": "tsx",
    ".js": "javascript",
    ".jsx": "javascript",
    ".mjs": "javascript",
    ".cjs": "javascript",
    ".php": "php",
    ".phtml": "php",
    ".py": "python",
}


class CorpusError(Exception):
    pass


@dataclass(frozen=True)
class Diff:
    id: str
    source: str
    repo: str | None
    sha: str | None
    parent: str | None
    licence: str
    language: str
    url: str
    files: list[str]


def language_of(path: str) -> str | None:
    dot = path.rfind(".")
    if dot < 0:
        return None
    return LANGUAGE_OF_EXT.get(path[dot:].lower())


def _load_one(path: Path) -> Diff:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as e:
        raise CorpusError(f"{path.name}: not valid JSON: {e}") from e
    required = ["id", "source", "repo", "sha", "parent", "licence", "language", "url", "files"]
    missing = [k for k in required if k not in raw]
    if missing:
        raise CorpusError(f"{path.name}: missing {', '.join(missing)}")
    if raw["id"] != path.stem:
        raise CorpusError(f"{path.name}: id {raw['id']!r} does not match the file name")
    if raw["source"] not in {"tree", "git"}:
        raise CorpusError(f"{path.name}: source must be tree or git")
    if raw["licence"] not in ALLOWED_LICENCES:
        raise CorpusError(f"{path.name}: licence {raw['licence']!r} is not one of {sorted(ALLOWED_LICENCES)}")
    if raw["language"] not in LANGUAGES:
        raise CorpusError(f"{path.name}: language {raw['language']!r} is not one of {sorted(LANGUAGES)}")
    if not isinstance(raw["files"], list) or not all(isinstance(f, str) and "\\" not in f for f in raw["files"]):
        raise CorpusError(f"{path.name}: files must be a list of forward-slash paths")
    if raw["source"] == "git":
        for k in ("repo", "sha", "parent"):
            if not isinstance(raw[k], str) or not raw[k]:
                raise CorpusError(f"{path.name}: git source needs {k}")
    else:
        for sub in ("before", "after"):
            if not (path.parent / raw["id"] / sub).is_dir():
                raise CorpusError(f"{path.name}: tree source needs {raw['id']}/{sub}/")
    return Diff(
        id=raw["id"], source=raw["source"], repo=raw["repo"], sha=raw["sha"], parent=raw["parent"],
        licence=raw["licence"], language=raw["language"], url=raw["url"], files=list(raw["files"]),
    )


def load_corpus(root: Path) -> list[Diff]:
    root = Path(root)
    if not root.is_dir():
        raise CorpusError(f"corpus directory {root} does not exist")
    return sorted((_load_one(p) for p in root.glob("*.json")), key=lambda d: d.id)
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python -m pytest tests/test_corpus.py -q`
Expected: `6 passed`.

- [ ] **Step 6: Commit**

```bash
git add LICENSE README.md CONTRIBUTING.md .gitignore pytest.ini bench/__init__.py bench/corpus.py tests/test_corpus.py
git commit -m "bench: repository scaffold and validated corpus records"
```

---

### Task 2: The fixture corpus (ten synthetic diffs)

**Files:**
- Create: `fixtures/corpus/<id>.json` and `fixtures/corpus/<id>/before/...`, `fixtures/corpus/<id>/after/...` for ten ids
- Test: `tests/test_fixtures.py`

**Interfaces:**
- Consumes: `bench.corpus.load_corpus`.
- Produces: ten `tree` records whose ids and expected rule hits later tasks rely on (listed below). Each fixture is a whole tiny repository with a `package.json` (name only) so the engine treats it as a project; `after/` differs from `before/` by the files named in `files`.

The ten fixtures and what each is for. Every "after" file is written to trigger exactly the rule named, with a shape the engine's own fixtures already use; the implementer checks each one by running `locrin check --sarif` inside a throwaway copy of `after/` before committing (Locrin 0.5.0 is installed on the machine as `locrin`).

| id | language | files changed | rule expected | notes |
|---|---|---|---|---|
| `fx-01-debug` | typescript | `src/a.ts` | `leftover-debug` | adds `console.log("debug", x)` inside a function |
| `fx-02-unused-import` | typescript | `src/b.ts` | `unused-import` | adds `import { join } from "path";` never used |
| `fx-03-unreachable` | typescript | `src/c.ts` | `unreachable` | a statement after `return` inside a function |
| `fx-04-marker` | javascript | `lib/d.js` | `leftover-agent-marker` | adds a `// TODO: implement` line and `// FIXME` |
| `fx-05-secret` | typescript | `src/e.ts` | `secret-exposed` | adds `const key = "AKIA" + "IOSFODNN7EXAMPLE";` split so the fixture never carries a matching literal; then a second line with a synthetic Stripe-shaped `sk_test_` value of 24 lowercase letters and digits, which the engine flags and which is not a real key |
| `fx-06-weak-crypto` | typescript | `src/f.ts` | `weak-crypto` | `crypto.createHash("md5")` |
| `fx-07-test-no-assert` | typescript | `src/g.test.ts` | `test-no-assert` | a `test("x", () => { run(); })` with no expect |
| `fx-08-php-debug` | php | `app/h.php` | `leftover-debug` | adds `var_dump($x);` |
| `fx-09-py-debug` | python | `pkg/i.py` | `leftover-debug` | adds `breakpoint()` |
| `fx-10-clean` | typescript | `src/j.ts` | none | a tidy change: no finding expected, so every reported finding on it is a false positive |

One fixture deliberately produces a second, wrong finding so the scorer's exact numbers are not all 1.0: `fx-01-debug` also adds `// TODO(agent): remove` on the line above the log, which `leftover-agent-marker` reports and which the label marks `false-positive` (the label says it is a real marker but the fixture labels it false to exercise the arithmetic). And `fx-03-unreachable` is labelled with one `missed` entry for `unreachable` on a second dead statement the engine collapses into the first, so recall for `unreachable` on fixtures is 1 of 2. Task 5's labels encode exactly these; Task 6's test asserts the resulting numbers.

- [ ] **Step 1: Write the fixture check test**

`tests/test_fixtures.py`:
```python
from pathlib import Path

from bench.corpus import load_corpus

FIXTURES = Path(__file__).resolve().parent.parent / "fixtures" / "corpus"


def test_ten_tree_fixtures_load():
    diffs = load_corpus(FIXTURES)
    assert len(diffs) == 10
    assert all(d.source == "tree" for d in diffs)
    assert [d.id for d in diffs][:2] == ["fx-01-debug", "fx-02-unused-import"]


def test_every_changed_file_differs_between_before_and_after():
    for d in load_corpus(FIXTURES):
        for f in d.files:
            before = FIXTURES / d.id / "before" / f
            after = FIXTURES / d.id / "after" / f
            assert after.is_file(), f"{d.id}: {f} missing in after"
            if before.exists():
                assert before.read_bytes() != after.read_bytes(), f"{d.id}: {f} unchanged"


def test_fixture_languages_cover_three_languages():
    langs = {d.language for d in load_corpus(FIXTURES)}
    assert {"typescript", "php", "python"} <= langs
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest tests/test_fixtures.py -q`
Expected: FAIL, `CorpusError: corpus directory ... does not exist`.

- [ ] **Step 3: Create the ten fixtures**

Each record, for example `fixtures/corpus/fx-01-debug.json`:
```json
{
  "id": "fx-01-debug",
  "source": "tree",
  "repo": null,
  "sha": null,
  "parent": null,
  "licence": "MIT",
  "language": "typescript",
  "url": "fixture",
  "files": ["src/a.ts"]
}
```

Each `before/` and `after/` tree contains `package.json` (`{"name": "fx-01-debug", "private": true}`) and the source files. `fx-01-debug/before/src/a.ts`:
```ts
export function total(xs: number[]): number {
  let sum = 0;
  for (const x of xs) sum += x;
  return sum;
}
```
`fx-01-debug/after/src/a.ts`:
```ts
export function total(xs: number[]): number {
  let sum = 0;
  for (const x of xs) sum += x;
  // TODO(agent): remove
  console.log("debug", sum);
  return sum;
}
```

`fx-08-php-debug/after/app/h.php`:
```php
<?php
function total(array $xs): int {
    $sum = 0;
    foreach ($xs as $x) { $sum += $x; }
    var_dump($sum);
    return $sum;
}
```
with `before/app/h.php` the same file without the `var_dump` line. The PHP and Python fixtures also carry `package.json` so the root is a project, and the harness enables `php` and `python` in every run (Task 4).

`fx-09-py-debug/after/pkg/i.py`:
```python
def total(xs):
    s = 0
    for x in xs:
        s += x
    breakpoint()
    return s
```

`fx-10-clean/after/src/j.ts` renames a local variable in a two-line function; `before` has the old name.

The remaining fixtures follow the table. For each, the implementer runs, from a temp copy of `after/` with `locrin.toml` containing `[languages]\nphp = true\npython = true` and the rule enables from Task 4's `BENCH_TOML`:
```bash
LOCRIN_CACHE_DIR=/tmp/lc locrin check --sarif --offline | python -c "import sys,json; [print(r['ruleId'], r['locations'][0]['physicalLocation']['artifactLocation']['uri'], r['locations'][0]['physicalLocation']['region']['startLine']) for r in json.load(sys.stdin)['runs'][0]['results']]"
```
and records in the task report the exact rule, file and line each fixture produced. Those lines feed Task 5's labels. If a fixture does not produce its rule, adjust the fixture until it does; do not adjust the label to the engine.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python -m pytest tests/test_fixtures.py tests/test_corpus.py -q`
Expected: `9 passed`.

- [ ] **Step 5: Commit**

```bash
git add fixtures/corpus tests/test_fixtures.py
git commit -m "bench: ten synthetic fixture diffs across TypeScript, JavaScript, PHP and Python"
```

---

### Task 3: Materialise a diff into a checkout

**Files:**
- Create: `bench/materialise.py`
- Test: `tests/test_materialise.py`

**Interfaces:**
- Consumes: `bench.corpus.Diff`.
- Produces: `materialise(diff: Diff, corpus_root: Path, cache: Path) -> Checkout` where `Checkout` is a dataclass `root: Path`, `base_ref: str`. For `tree` sources it builds a fresh git repository under `cache/tree/<id>/` with two commits (before, after) and returns `base_ref` = the before commit sha, working tree at after. For `git` sources it clones `https://github.com/<repo>.git` with `--filter=blob:none` into `cache/repos/<owner>__<name>/` once, then `git checkout --detach <sha>` and returns `base_ref` = `parent`. It removes any `locrin.toml` and `locrin-baseline.json` the checkout carries and records that in `Checkout.removed: list[str]`. `MaterialiseError(Exception)` on any git failure, naming the diff.
- All git calls go through one helper `_git(args: list[str], cwd: Path) -> str` using `subprocess.run` with `check=True`, `text=True`, captured output, and the environment variables `GIT_AUTHOR_NAME=bench GIT_AUTHOR_EMAIL=bench@example.invalid GIT_COMMITTER_NAME=bench GIT_COMMITTER_EMAIL=bench@example.invalid` so commits are reproducible.

- [ ] **Step 1: Write the failing tests**

`tests/test_materialise.py`:
```python
import subprocess
from pathlib import Path

import pytest

from bench.corpus import Diff
from bench.materialise import Checkout, MaterialiseError, materialise


def tree_diff(tmp_path: Path) -> tuple[Diff, Path]:
    corpus = tmp_path / "corpus"
    (corpus / "fx-x" / "before" / "src").mkdir(parents=True)
    (corpus / "fx-x" / "after" / "src").mkdir(parents=True)
    (corpus / "fx-x" / "before" / "src" / "a.ts").write_text("export const a = 1;\n")
    (corpus / "fx-x" / "after" / "src" / "a.ts").write_text("export const a = 2;\n")
    (corpus / "fx-x" / "after" / "locrin.toml").write_text("[languages]\nphp = true\n")
    d = Diff(id="fx-x", source="tree", repo=None, sha=None, parent=None, licence="MIT",
             language="typescript", url="fixture", files=["src/a.ts"])
    return d, corpus


def test_tree_source_builds_two_commit_repo(tmp_path):
    d, corpus = tree_diff(tmp_path)
    co = materialise(d, corpus, tmp_path / "cache")
    assert isinstance(co, Checkout)
    assert (co.root / "src" / "a.ts").read_text() == "export const a = 2;\n"
    log = subprocess.run(["git", "log", "--format=%s"], cwd=co.root, capture_output=True, text=True, check=True).stdout.split()
    assert log == ["after", "before"]
    base = subprocess.run(["git", "show", f"{co.base_ref}:src/a.ts"], cwd=co.root, capture_output=True, text=True, check=True).stdout
    assert base == "export const a = 1;\n"


def test_tree_source_removes_engine_config_files(tmp_path):
    d, corpus = tree_diff(tmp_path)
    co = materialise(d, corpus, tmp_path / "cache")
    assert not (co.root / "locrin.toml").exists()
    assert co.removed == ["locrin.toml"]


def test_tree_source_is_idempotent(tmp_path):
    d, corpus = tree_diff(tmp_path)
    a = materialise(d, corpus, tmp_path / "cache")
    b = materialise(d, corpus, tmp_path / "cache")
    assert a.base_ref == b.base_ref


def test_git_source_uses_cached_clone(tmp_path, monkeypatch):
    calls: list[list[str]] = []

    def fake_git(args, cwd):
        calls.append(list(args))
        if args[:2] == ["clone", "--filter=blob:none"]:
            Path(args[-1]).mkdir(parents=True, exist_ok=True)
        return ""

    monkeypatch.setattr("bench.materialise._git", fake_git)
    d = Diff(id="acme__w__abc1234", source="git", repo="acme/w", sha="abc1234" * 5 + "abcde",
             parent="def5678" * 5 + "defgh", licence="MIT", language="typescript",
             url="https://github.com/acme/w/commit/abc1234", files=["src/a.ts"])
    co = materialise(d, tmp_path / "corpus", tmp_path / "cache")
    assert co.base_ref == d.parent
    assert co.root == tmp_path / "cache" / "repos" / "acme__w"
    assert calls[0][:2] == ["clone", "--filter=blob:none"]
    assert calls[0][2] == "https://github.com/acme/w.git"
    materialise(d, tmp_path / "corpus", tmp_path / "cache")
    assert sum(1 for c in calls if c[0] == "clone") == 1
    assert ["checkout", "--detach", d.sha] in calls


def test_git_failure_names_the_diff(tmp_path, monkeypatch):
    def boom(args, cwd):
        raise subprocess.CalledProcessError(128, args, stderr="fatal: bad object")

    monkeypatch.setattr("bench.materialise._git", boom)
    d = Diff(id="acme__w__abc1234", source="git", repo="acme/w", sha="a" * 40, parent="b" * 40,
             licence="MIT", language="typescript", url="u", files=[])
    with pytest.raises(MaterialiseError, match="acme__w__abc1234"):
        materialise(d, tmp_path / "corpus", tmp_path / "cache")
```

- [ ] **Step 2: Run them to verify they fail**

Run: `python -m pytest tests/test_materialise.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.materialise'`.

- [ ] **Step 3: Write the implementation**

`bench/materialise.py`:
```python
"""Turn a corpus record into a git checkout with a base ref for locrin check --base."""
from __future__ import annotations

import os
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from bench.corpus import Diff

ENGINE_FILES = ("locrin.toml", "locrin-baseline.json")
_ENV = {
    "GIT_AUTHOR_NAME": "bench",
    "GIT_AUTHOR_EMAIL": "bench@example.invalid",
    "GIT_COMMITTER_NAME": "bench",
    "GIT_COMMITTER_EMAIL": "bench@example.invalid",
    "GIT_TERMINAL_PROMPT": "0",
}


class MaterialiseError(Exception):
    pass


@dataclass
class Checkout:
    root: Path
    base_ref: str
    removed: list[str] = field(default_factory=list)


def _git(args: list[str], cwd: Path) -> str:
    env = dict(os.environ)
    env.update(_ENV)
    return subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True, text=True).stdout


def _copy_tree(src: Path, dst: Path) -> None:
    for p in src.rglob("*"):
        if p.is_file():
            target = dst / p.relative_to(src)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(p, target)


def _clear_worktree(root: Path) -> None:
    for p in root.iterdir():
        if p.name == ".git":
            continue
        shutil.rmtree(p) if p.is_dir() else p.unlink()


def _strip_engine_files(root: Path) -> list[str]:
    removed = []
    for name in ENGINE_FILES:
        p = root / name
        if p.exists():
            p.unlink()
            removed.append(name)
    return removed


def _materialise_tree(diff: Diff, corpus_root: Path, cache: Path) -> Checkout:
    src = corpus_root / diff.id
    root = cache / "tree" / diff.id
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    _git(["init", "-q", "-b", "main"], root)
    _copy_tree(src / "before", root)
    _strip_engine_files(root)
    _git(["add", "-A"], root)
    _git(["commit", "-q", "-m", "before", "--allow-empty"], root)
    base = _git(["rev-parse", "HEAD"], root).strip()
    _clear_worktree(root)
    _copy_tree(src / "after", root)
    removed = _strip_engine_files(root)
    _git(["add", "-A"], root)
    _git(["commit", "-q", "-m", "after", "--allow-empty"], root)
    return Checkout(root=root, base_ref=base, removed=removed)


def _materialise_git(diff: Diff, cache: Path) -> Checkout:
    owner, name = diff.repo.split("/", 1)
    root = cache / "repos" / f"{owner}__{name}"
    if not root.exists():
        root.parent.mkdir(parents=True, exist_ok=True)
        _git(["clone", "--filter=blob:none", f"https://github.com/{diff.repo}.git", str(root)], root.parent)
    _git(["checkout", "--detach", diff.sha], root)
    _git(["clean", "-fdq"], root)
    removed = _strip_engine_files(root)
    return Checkout(root=root, base_ref=diff.parent, removed=removed)


def materialise(diff: Diff, corpus_root: Path, cache: Path) -> Checkout:
    try:
        if diff.source == "tree":
            return _materialise_tree(diff, Path(corpus_root), Path(cache))
        return _materialise_git(diff, Path(cache))
    except subprocess.CalledProcessError as e:
        raise MaterialiseError(f"{diff.id}: git {' '.join(e.cmd[1:] if isinstance(e.cmd, list) else [str(e.cmd)])} failed: {e.stderr or e.stdout}") from e
```

Note for the implementer: `git add -A` here is inside a throwaway repository the harness owns; the rule against `git add -A` is about the founder's repositories and this is not one.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python -m pytest tests/test_materialise.py -q`
Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add bench/materialise.py tests/test_materialise.py
git commit -m "bench: materialise tree and git diffs into checkouts with a base ref"
```

---

### Task 4: Install Locrin, run check, normalise SARIF

**Files:**
- Create: `bench/run.py`, `fixtures/sarif/sample.sarif`
- Test: `tests/test_run.py`

**Interfaces:**
- Consumes: `bench.corpus.Diff`, `bench.corpus.language_of`, `bench.materialise.Checkout`.
- Produces:
  - `BENCH_TOML: str`, the config written into every checkout:
    ```toml
    # Written by locrin-benchmark. Every measurable rule on, every language on.
    [languages]
    php = true
    python = true

    [rules.dead-file]
    enabled = true
    [rules.swallowed-error]
    enabled = true
    [rules.injection-sink]
    enabled = true
    [rules.leftover-commented-code]
    languages = ["typescript", "tsx", "javascript", "php", "python"]
    ```
  - `install_locrin(version: str, cache: Path) -> Path`: returns `cache/bin/<version>/locrin` (or `locrin.exe`); if absent, downloads `https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh` with `urllib` and runs `bash install.sh <version>` with `LOCRIN_INSTALL_DIR` set to that directory. If the environment variable `LOCRIN_BIN` is set, returns it and installs nothing (for local development and CI with a prebuilt binary). Verifies with `locrin --version` that the output ends with the version without its `v`.
  - `Finding` dataclass: `diff: str`, `rule: str`, `file: str`, `line: int`, `id: str`, `confidence: str`, `language: str | None`.
  - `normalise(diff_id: str, sarif: dict) -> tuple[list[Finding], dict[str, RuleMeta]]` where `RuleMeta` is `id: str`, `enabled_by_default: bool`, `languages: list[str]`.
  - `run_check(locrin: Path, checkout: Checkout, work: Path) -> dict`: writes `BENCH_TOML` to `checkout.root / "locrin.toml"`, runs `[locrin, "check", "--root", root, "--base", base_ref, "--sarif", "--offline"]` with `LOCRIN_CACHE_DIR=work/cache/<diff id>` and `cwd=root`, accepts exit 0 and 1, raises `RunError` naming the diff on exit 2 or on unparsable output, returns the parsed SARIF document.
  - `RunError(Exception)`.

- [ ] **Step 1: Capture a SARIF sample**

Run Locrin 0.5.0 on the `fx-01-debug` fixture (a temp copy of `after/` with `BENCH_TOML` written as `locrin.toml`):
```bash
LOCRIN_CACHE_DIR=/tmp/lc locrin check --sarif --offline > fixtures/sarif/sample.sarif
```
Confirm it contains at least the `leftover-debug` and `leftover-agent-marker` results and the `tool.driver.rules` list. Exit code 1 is expected (a high-confidence finding blocks); the redirect still captured stdout.

- [ ] **Step 2: Write the failing tests**

`tests/test_run.py`:
```python
import json
import subprocess
from pathlib import Path

import pytest

from bench.materialise import Checkout
from bench.run import BENCH_TOML, Finding, RunError, install_locrin, normalise, run_check

SAMPLE = Path(__file__).resolve().parent.parent / "fixtures" / "sarif" / "sample.sarif"


def test_normalise_extracts_rule_file_line_id_and_language():
    doc = json.loads(SAMPLE.read_text(encoding="utf-8"))
    findings, rules = normalise("fx-01-debug", doc)
    debug = [f for f in findings if f.rule == "leftover-debug"]
    assert len(debug) == 1
    f = debug[0]
    assert isinstance(f, Finding)
    assert (f.diff, f.file, f.language) == ("fx-01-debug", "src/a.ts", "typescript")
    assert f.line == 5 and len(f.id) == 16 and f.confidence == "high"
    assert rules["dead-file"].enabled_by_default is False
    assert rules["leftover-debug"].enabled_by_default is True
    assert "php" in rules["leftover-debug"].languages
    assert "php" not in rules["unused-import"].languages


def test_normalise_orders_findings_deterministically():
    doc = json.loads(SAMPLE.read_text(encoding="utf-8"))
    findings, _ = normalise("fx-01-debug", doc)
    keys = [(f.rule, f.file, f.line, f.id) for f in findings]
    assert keys == sorted(keys)


def test_install_uses_locrin_bin_when_set(tmp_path, monkeypatch):
    fake = tmp_path / "locrin"
    fake.write_text("#!/bin/sh\necho locrin 0.5.0\n")
    fake.chmod(0o755)
    monkeypatch.setenv("LOCRIN_BIN", str(fake))
    assert install_locrin("v0.5.0", tmp_path / "cache") == fake


def test_install_rejects_version_mismatch(tmp_path, monkeypatch):
    fake = tmp_path / "locrin"
    fake.write_text("#!/bin/sh\necho locrin 0.4.0\n")
    fake.chmod(0o755)
    monkeypatch.setenv("LOCRIN_BIN", str(fake))
    with pytest.raises(RunError, match="0.4.0"):
        install_locrin("v0.5.0", tmp_path / "cache")


def test_run_check_writes_config_accepts_exit_1_and_rejects_exit_2(tmp_path, monkeypatch):
    root = tmp_path / "co"
    root.mkdir()
    co = Checkout(root=root, base_ref="abc")
    seen = {}

    def fake_run(cmd, **kw):
        seen["cmd"] = cmd
        seen["env"] = kw["env"]
        code = fake_run.code
        return subprocess.CompletedProcess(cmd, code, stdout=json.dumps({"runs": [{"results": [], "tool": {"driver": {"rules": []}}}]}), stderr="")

    fake_run.code = 1
    monkeypatch.setattr("bench.run.subprocess.run", fake_run)
    doc = run_check(Path("/bin/locrin"), co, tmp_path / "work")
    assert doc["runs"][0]["results"] == []
    assert (root / "locrin.toml").read_text() == BENCH_TOML
    assert seen["cmd"][1:] == ["check", "--root", str(root), "--base", "abc", "--sarif", "--offline"]
    assert seen["env"]["LOCRIN_CACHE_DIR"].endswith("co") or "cache" in seen["env"]["LOCRIN_CACHE_DIR"]
    fake_run.code = 2
    with pytest.raises(RunError, match="exit 2"):
        run_check(Path("/bin/locrin"), co, tmp_path / "work")
```

- [ ] **Step 3: Run them to verify they fail**

Run: `python -m pytest tests/test_run.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.run'`.

- [ ] **Step 4: Write the implementation**

`bench/run.py`:
```python
"""Install a Locrin version, run check in SARIF mode, normalise the findings."""
from __future__ import annotations

import json
import os
import platform
import subprocess
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from bench.corpus import language_of
from bench.materialise import Checkout

INSTALLER = "https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh"

BENCH_TOML = """# Written by locrin-benchmark. Every measurable rule on, every language on.
[languages]
php = true
python = true

[rules.dead-file]
enabled = true
[rules.swallowed-error]
enabled = true
[rules.injection-sink]
enabled = true
[rules.leftover-commented-code]
languages = ["typescript", "tsx", "javascript", "php", "python"]
"""


class RunError(Exception):
    pass


@dataclass(frozen=True)
class Finding:
    diff: str
    rule: str
    file: str
    line: int
    id: str
    confidence: str
    language: str | None


@dataclass(frozen=True)
class RuleMeta:
    id: str
    enabled_by_default: bool
    languages: list[str]


def _version_of(binary: Path) -> str:
    out = subprocess.run([str(binary), "--version"], capture_output=True, text=True, check=True).stdout.strip()
    return out.split()[-1]


def install_locrin(version: str, cache: Path) -> Path:
    want = version[1:] if version.startswith("v") else version
    override = os.environ.get("LOCRIN_BIN")
    if override:
        binary = Path(override)
        got = _version_of(binary)
        if got != want:
            raise RunError(f"LOCRIN_BIN is locrin {got}, wanted {want}")
        return binary
    bin_dir = Path(cache) / "bin" / version
    binary = bin_dir / ("locrin.exe" if platform.system() == "Windows" else "locrin")
    if not binary.exists():
        bin_dir.mkdir(parents=True, exist_ok=True)
        script = bin_dir / "install.sh"
        with urllib.request.urlopen(INSTALLER, timeout=60) as r:
            script.write_bytes(r.read())
        env = dict(os.environ, LOCRIN_INSTALL_DIR=str(bin_dir))
        subprocess.run(["bash", str(script), version], env=env, check=True)
    got = _version_of(binary)
    if got != want:
        raise RunError(f"installed locrin {got}, wanted {want}")
    return binary


def normalise(diff_id: str, sarif: dict) -> tuple[list[Finding], dict[str, RuleMeta]]:
    run = sarif["runs"][0]
    findings = []
    for r in run.get("results", []):
        loc = r["locations"][0]["physicalLocation"]
        file = loc["artifactLocation"]["uri"]
        findings.append(Finding(
            diff=diff_id,
            rule=r["ruleId"],
            file=file,
            line=int(loc["region"]["startLine"]),
            id=r.get("partialFingerprints", {}).get("locrin/id", ""),
            confidence=str(r.get("properties", {}).get("confidence", "")).lower(),
            language=language_of(file),
        ))
    findings.sort(key=lambda f: (f.rule, f.file, f.line, f.id))
    rules = {}
    for r in run.get("tool", {}).get("driver", {}).get("rules", []):
        rules[r["id"]] = RuleMeta(
            id=r["id"],
            enabled_by_default=bool(r.get("defaultConfiguration", {}).get("enabled", True)),
            languages=list(r.get("properties", {}).get("languages", [])),
        )
    return findings, rules


def run_check(locrin: Path, checkout: Checkout, work: Path) -> dict:
    root = checkout.root
    (root / "locrin.toml").write_text(BENCH_TOML, encoding="utf-8")
    cache_dir = Path(work) / "cache" / root.name
    cache_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, LOCRIN_CACHE_DIR=str(cache_dir))
    cmd = [str(locrin), "check", "--root", str(root), "--base", checkout.base_ref, "--sarif", "--offline"]
    proc = subprocess.run(cmd, cwd=root, env=env, capture_output=True, text=True)
    if proc.returncode not in (0, 1):
        raise RunError(f"{root.name}: locrin exit {proc.returncode}: {proc.stderr.strip()[:500]}")
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as e:
        raise RunError(f"{root.name}: locrin printed no SARIF: {e}") from e
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python -m pytest tests/test_run.py -q`
Expected: `5 passed`. The line number asserted in the first test must match the captured sample; if the sample's `leftover-debug` line is not 5, fix the fixture so the log sits on line 5, recapture, and keep the test at 5.

- [ ] **Step 6: Commit**

```bash
git add bench/run.py fixtures/sarif/sample.sarif tests/test_run.py
git commit -m "bench: install locrin, run check in SARIF mode, normalise findings"
```

---

### Task 5: Labels, the label tool and the labelling protocol

**Files:**
- Create: `bench/labels.py`, `bench/label_tool.py`, `label.py`, `LABELLING.md`, `fixtures/labels/<id>.json` for all ten fixtures
- Test: `tests/test_labels.py`

**Interfaces:**
- Consumes: `bench.run.Finding`.
- Produces:
  - Label file schema, one per diff at `labels/<id>.json` (or `fixtures/labels/`):
    ```json
    {
      "diff": "fx-01-debug",
      "locrin": "v0.5.0",
      "pass1": {"by": "opus", "date": "2026-09-14"},
      "pass2": {"by": "fable", "date": "2026-09-14"},
      "entries": [
        {"rule": "leftover-debug", "file": "src/a.ts", "line": 5, "id": "0123456789abcdef", "pass1": "true", "pass2": "true", "note": ""},
        {"rule": "leftover-agent-marker", "file": "src/a.ts", "line": 4, "id": "fedcba9876543210", "pass1": "false-positive", "pass2": "false-positive", "note": "fixture arithmetic"},
        {"rule": "unreachable", "file": "src/c.ts", "line": 7, "id": null, "pass1": "missed", "pass2": "missed", "note": "second dead statement"}
      ]
    }
    ```
    Verdict values: `true`, `false-positive`, `missed`, `not-applicable`, or `?` (unfilled). `id` is the engine id for reported findings and `null` for `missed` entries.
  - `bench.labels`: `Entry` dataclass (`rule, file, line, id, pass1, pass2, note`), `LabelFile` dataclass (`diff, locrin, pass1, pass2, entries`), `load_labels(root: Path) -> dict[str, LabelFile]`, `LabelError`, `Entry.confirmed -> bool` (both passes filled, equal, and not `?`), `Entry.verdict -> str | None` (the agreed verdict or None), `disagreements(labels) -> list[tuple[str, Entry]]`.
  - `bench.label_tool`: `new(diff_id, findings, locrin_version, out_root, by)` writes a template with every finding as an entry with `pass1: "?"`, `pass2: "?"`; `confirm(diff_id, root, by, date)` fills `pass2` metadata after the second labeller edited the entries; `status(root)` prints counts of confirmed, unfilled and disagreeing entries per diff. The CLI `python label.py new <id> --locrin v0.5.0 --by opus`, `python label.py confirm <id> --by fable`, `python label.py status`. `new` runs the harness on that one diff (Tasks 3 and 4) to get the findings.

- [ ] **Step 1: Write LABELLING.md**

```markdown
# Labelling protocol

Every label is written twice, by two independent passes, and counts only when both agree. Pass one runs `python label.py new <id> --locrin <version> --by <name>`, which runs the engine on the diff and writes `labels/<id>.json` with every reported finding as an entry marked `?`. The labeller reads the diff and the code around each finding and sets `pass1` on every entry, then adds `missed` entries for anything a rule should have reported and did not. Pass two edits `pass2` on every entry without looking at `pass1` (the tool hides it when run with `--blind`), then runs `python label.py confirm <id> --by <name>`.

Verdicts:

- `true`: the rule's definition below is met at that file and line.
- `false-positive`: the engine reported it and the definition is not met.
- `missed`: the definition is met at that file and line and the engine did not report it. Use the first line of the offending construct.
- `not-applicable`: the finding is on a file the diff did not really change (a rename, a generated file, vendored code). Excluded from both precision and recall.

## What counts as true, per rule

- `leftover-debug`: a debug print or breakpoint left in non-script code (console.log, debugger, var_dump, dd, breakpoint(), pdb). A log statement that is the module's real logging is not true; a print in a CLI script's main path is not true.
- `leftover-commented-code`: a comment whose content is code that was disabled, not prose describing code.
- `leftover-agent-marker`: TODO, FIXME, XXX, HACK or an explicit agent marker left in the change. A marker in a path or file name is not true.
- `unused-import`: an import whose every binding is unused in the file after the change.
- `unreachable`: a statement after return, throw, break or continue in the same block, or in a branch whose condition is a literal.
- `dead-export`: an exported symbol with no importer anywhere in the repository and not an entry point.
- `dead-file`: a source file no other file imports and no entry point names.
- `swallowed-error`: a catch or except block that neither rethrows, logs, returns an error value, nor is documented as intentional.
- `test-no-assert`: a test body with no assertion or expectation call.
- `test-newly-skipped`: a test the change marked skip, only, todo or xfail.
- `secret-exposed`: a real-looking credential literal (provider-shaped key, private key block, password in an assignment). Documented placeholder formats from a provider's docs are not true.
- `weak-crypto`: md5, sha1 for security purposes, DES, RC4, ECB, Math.random for tokens.
- `injection-sink`: user-controlled input reaching a query, shell, eval or HTML sink without a parameterisation or escape.
- `html-injection`: unescaped interpolation into innerHTML, dangerouslySetInnerHTML or a template rendered as HTML.
- `supabase-service-role-in-client`, `supabase-table-without-rls`, `express-cors-wildcard-on-authenticated`, `express-cookie-insecure`: the named construct in code that runs on the client (or the server, for the Express pair) exactly as the rule name says.

Not labelled: `vulnerable-dependency` (advisory feed), `boundary-violation` and `express-route-without-auth` (need per-repository config). Entries for these rules are set to `not-applicable`.

## Disputes

An entry where the two passes disagree is listed by `python label.py status` and excluded from every number until a maintainer settles it by editing both passes with a note that says why.
```

- [ ] **Step 2: Write the failing tests**

`tests/test_labels.py`:
```python
import json
from pathlib import Path

import pytest

from bench.label_tool import confirm, new, status
from bench.labels import LabelError, disagreements, load_labels
from bench.run import Finding

FIXTURE_LABELS = Path(__file__).resolve().parent.parent / "fixtures" / "labels"


def entry(**over):
    e = {"rule": "leftover-debug", "file": "src/a.ts", "line": 5, "id": "0" * 16, "pass1": "true", "pass2": "true", "note": ""}
    e.update(over)
    return e


def label(tmp_path, entries, diff="fx-01-debug"):
    rec = {"diff": diff, "locrin": "v0.5.0", "pass1": {"by": "a", "date": "2026-09-14"},
           "pass2": {"by": "b", "date": "2026-09-14"}, "entries": entries}
    (tmp_path / f"{diff}.json").write_text(json.dumps(rec), encoding="utf-8")


def test_confirmed_only_when_both_passes_agree(tmp_path):
    label(tmp_path, [entry(), entry(line=9, pass2="false-positive"), entry(line=11, pass1="?", pass2="?")])
    labels = load_labels(tmp_path)
    es = labels["fx-01-debug"].entries
    assert [e.confirmed for e in es] == [True, False, False]
    assert es[0].verdict == "true" and es[1].verdict is None
    assert [(d, e.line) for d, e in disagreements(labels)] == [("fx-01-debug", 9)]


def test_missed_entries_have_null_id_and_reported_ones_do_not(tmp_path):
    label(tmp_path, [entry(id=None, pass1="missed", pass2="missed")])
    assert load_labels(tmp_path)["fx-01-debug"].entries[0].id is None
    label(tmp_path, [entry(id=None)])
    with pytest.raises(LabelError, match="id"):
        load_labels(tmp_path)


def test_rejects_unknown_verdict_and_diff_mismatch(tmp_path):
    label(tmp_path, [entry(pass1="maybe")])
    with pytest.raises(LabelError, match="verdict"):
        load_labels(tmp_path)
    label(tmp_path, [entry()], diff="wrong")
    (tmp_path / "fx-01-debug.json").unlink()
    (tmp_path / "wrong.json").rename(tmp_path / "fx-01-debug.json")
    with pytest.raises(LabelError, match="diff"):
        load_labels(tmp_path)


def test_new_writes_template_with_unfilled_passes(tmp_path):
    fs = [Finding("fx-01-debug", "leftover-debug", "src/a.ts", 5, "1" * 16, "high", "typescript")]
    new("fx-01-debug", fs, "v0.5.0", tmp_path, by="opus", date="2026-09-14")
    rec = json.loads((tmp_path / "fx-01-debug.json").read_text())
    assert rec["pass1"] == {"by": "opus", "date": "2026-09-14"} and rec["pass2"] is None
    assert rec["entries"] == [{"rule": "leftover-debug", "file": "src/a.ts", "line": 5, "id": "1" * 16, "pass1": "?", "pass2": "?", "note": ""}]


def test_confirm_sets_pass2_metadata_and_refuses_unfilled(tmp_path):
    fs = [Finding("fx-01-debug", "leftover-debug", "src/a.ts", 5, "1" * 16, "high", "typescript")]
    new("fx-01-debug", fs, "v0.5.0", tmp_path, by="opus", date="2026-09-14")
    with pytest.raises(LabelError, match="unfilled"):
        confirm("fx-01-debug", tmp_path, by="fable", date="2026-09-15")
    p = tmp_path / "fx-01-debug.json"
    rec = json.loads(p.read_text())
    rec["entries"][0]["pass1"] = "true"
    rec["entries"][0]["pass2"] = "true"
    p.write_text(json.dumps(rec))
    confirm("fx-01-debug", tmp_path, by="fable", date="2026-09-15")
    assert json.loads(p.read_text())["pass2"] == {"by": "fable", "date": "2026-09-15"}


def test_fixture_labels_load_and_are_all_confirmed():
    labels = load_labels(FIXTURE_LABELS)
    assert len(labels) == 10
    assert disagreements(labels) == []
    assert all(e.confirmed for lf in labels.values() for e in lf.entries)


def test_status_counts(tmp_path, capsys):
    label(tmp_path, [entry(), entry(line=9, pass2="false-positive"), entry(line=11, pass1="?", pass2="?")])
    status(tmp_path)
    out = capsys.readouterr().out
    assert "fx-01-debug" in out and "confirmed=1" in out and "unfilled=1" in out and "disagree=1" in out
```

- [ ] **Step 3: Run them to verify they fail**

Run: `python -m pytest tests/test_labels.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.label_tool'`.

- [ ] **Step 4: Write the implementation**

`bench/labels.py`:
```python
"""Label files: two independent passes per entry; an entry counts only when they agree."""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

VERDICTS = {"true", "false-positive", "missed", "not-applicable", "?"}


class LabelError(Exception):
    pass


@dataclass
class Entry:
    rule: str
    file: str
    line: int
    id: str | None
    pass1: str
    pass2: str
    note: str

    @property
    def confirmed(self) -> bool:
        return self.pass1 != "?" and self.pass1 == self.pass2

    @property
    def verdict(self) -> str | None:
        return self.pass1 if self.confirmed else None


@dataclass
class LabelFile:
    diff: str
    locrin: str
    pass1: dict | None
    pass2: dict | None
    entries: list[Entry]


def _entry(name: str, i: int, raw: dict) -> Entry:
    for k in ("rule", "file", "line", "pass1", "pass2"):
        if k not in raw:
            raise LabelError(f"{name}: entry {i} missing {k}")
    for k in ("pass1", "pass2"):
        if raw[k] not in VERDICTS:
            raise LabelError(f"{name}: entry {i} has unknown verdict {raw[k]!r}")
    ident = raw.get("id")
    is_missed = "missed" in (raw["pass1"], raw["pass2"])
    if ident is None and not is_missed:
        raise LabelError(f"{name}: entry {i} needs an id unless it is missed")
    if ident is not None and (not isinstance(ident, str) or len(ident) != 16):
        raise LabelError(f"{name}: entry {i} id must be 16 hex characters")
    return Entry(rule=raw["rule"], file=raw["file"], line=int(raw["line"]), id=ident,
                 pass1=raw["pass1"], pass2=raw["pass2"], note=str(raw.get("note", "")))


def load_label_file(path: Path) -> LabelFile:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if raw.get("diff") != path.stem:
        raise LabelError(f"{path.name}: diff {raw.get('diff')!r} does not match the file name")
    entries = [_entry(path.name, i, e) for i, e in enumerate(raw.get("entries", []))]
    return LabelFile(diff=raw["diff"], locrin=raw.get("locrin", ""), pass1=raw.get("pass1"), pass2=raw.get("pass2"), entries=entries)


def load_labels(root: Path) -> dict[str, LabelFile]:
    root = Path(root)
    return {p.stem: load_label_file(p) for p in sorted(root.glob("*.json"))}


def disagreements(labels: dict[str, LabelFile]) -> list[tuple[str, Entry]]:
    out = []
    for diff, lf in labels.items():
        for e in lf.entries:
            if e.pass1 != "?" and e.pass2 != "?" and e.pass1 != e.pass2:
                out.append((diff, e))
    return out


def save_label_file(root: Path, lf: LabelFile) -> None:
    raw = {
        "diff": lf.diff, "locrin": lf.locrin, "pass1": lf.pass1, "pass2": lf.pass2,
        "entries": [{"rule": e.rule, "file": e.file, "line": e.line, "id": e.id, "pass1": e.pass1, "pass2": e.pass2, "note": e.note} for e in lf.entries],
    }
    (Path(root) / f"{lf.diff}.json").write_text(json.dumps(raw, indent=2) + "\n", encoding="utf-8")
```

`bench/label_tool.py`:
```python
"""label.py: new, confirm, status."""
from __future__ import annotations

import argparse
import datetime as dt
import sys
from pathlib import Path

from bench.labels import Entry, LabelError, LabelFile, disagreements, load_label_file, load_labels, save_label_file
from bench.run import Finding


def new(diff_id: str, findings: list[Finding], locrin_version: str, out_root: Path, by: str, date: str) -> Path:
    entries = [Entry(rule=f.rule, file=f.file, line=f.line, id=f.id, pass1="?", pass2="?", note="") for f in findings]
    lf = LabelFile(diff=diff_id, locrin=locrin_version, pass1={"by": by, "date": date}, pass2=None, entries=entries)
    Path(out_root).mkdir(parents=True, exist_ok=True)
    save_label_file(out_root, lf)
    return Path(out_root) / f"{diff_id}.json"


def confirm(diff_id: str, root: Path, by: str, date: str) -> None:
    lf = load_label_file(Path(root) / f"{diff_id}.json")
    unfilled = [e for e in lf.entries if e.pass1 == "?" or e.pass2 == "?"]
    if unfilled:
        raise LabelError(f"{diff_id}: {len(unfilled)} unfilled entries; fill pass1 and pass2 before confirming")
    lf.pass2 = {"by": by, "date": date}
    save_label_file(root, lf)


def status(root: Path) -> None:
    labels = load_labels(root)
    dis = {(d, e.line, e.rule) for d, e in disagreements(labels)}
    for diff, lf in labels.items():
        confirmed = sum(1 for e in lf.entries if e.confirmed)
        unfilled = sum(1 for e in lf.entries if e.pass1 == "?" or e.pass2 == "?")
        disagree = sum(1 for e in lf.entries if (diff, e.line, e.rule) in dis)
        print(f"{diff}: entries={len(lf.entries)} confirmed={confirmed} unfilled={unfilled} disagree={disagree}")


def _findings_for(diff_id: str, locrin_version: str, corpus_root: Path, work: Path) -> list[Finding]:
    from bench.corpus import load_corpus
    from bench.materialise import materialise
    from bench.run import install_locrin, normalise, run_check
    diff = next((d for d in load_corpus(corpus_root) if d.id == diff_id), None)
    if diff is None:
        raise LabelError(f"{diff_id}: not in {corpus_root}")
    locrin = install_locrin(locrin_version, work / ".cache")
    co = materialise(diff, corpus_root, work / ".cache")
    findings, _ = normalise(diff_id, run_check(locrin, co, work / ".work"))
    return findings


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="label.py")
    sub = p.add_subparsers(dest="cmd", required=True)
    n = sub.add_parser("new")
    n.add_argument("diff")
    n.add_argument("--locrin", required=True)
    n.add_argument("--by", required=True)
    n.add_argument("--corpus", default="corpus")
    n.add_argument("--labels", default="labels")
    c = sub.add_parser("confirm")
    c.add_argument("diff")
    c.add_argument("--by", required=True)
    c.add_argument("--labels", default="labels")
    s = sub.add_parser("status")
    s.add_argument("--labels", default="labels")
    a = p.parse_args(argv)
    today = dt.date.today().isoformat()
    try:
        if a.cmd == "new":
            findings = _findings_for(a.diff, a.locrin, Path(a.corpus), Path("."))
            print(new(a.diff, findings, a.locrin, Path(a.labels), a.by, today))
        elif a.cmd == "confirm":
            confirm(a.diff, Path(a.labels), a.by, today)
            print(f"{a.diff}: pass two recorded")
        else:
            status(Path(a.labels))
    except LabelError as e:
        print(f"label.py: {e}", file=sys.stderr)
        return 1
    return 0
```

`label.py` at the repository root:
```python
import sys

from bench.label_tool import main

sys.exit(main())
```

- [ ] **Step 5: Write the ten fixture labels**

One file per fixture in `fixtures/labels/`, using the exact rule, file, line and id values recorded in Task 2's report (run the engine again if the report lacks an id: `python label.py new fx-01-debug --locrin v0.5.0 --by opus --corpus fixtures/corpus --labels /tmp/x` prints the template with ids). Both passes filled and equal, `pass1.by = "opus"`, `pass2.by = "fable"`, dates `2026-09-14`. The verdicts, so that Task 6's numbers come out exactly:

- `fx-01-debug`: `leftover-debug` src/a.ts line 5 `true`; `leftover-agent-marker` src/a.ts line 4 `false-positive` (note: "fixture arithmetic").
- `fx-02-unused-import`: `unused-import` `true`.
- `fx-03-unreachable`: `unreachable` (reported) `true`; a second entry `unreachable` src/c.ts at the line of the second dead statement, `id: null`, `missed`.
- `fx-04-marker`: both `leftover-agent-marker` findings `true`.
- `fx-05-secret`: `secret-exposed` `true` (the engine reports the class once per file; one entry).
- `fx-06-weak-crypto`: `weak-crypto` `true`.
- `fx-07-test-no-assert`: `test-no-assert` `true`.
- `fx-08-php-debug`: `leftover-debug` app/h.php `true`.
- `fx-09-py-debug`: `leftover-debug` pkg/i.py `true`.
- `fx-10-clean`: no entries (`"entries": []`).

Any extra finding the engine reports on a fixture that is not in this list gets an entry marked `not-applicable` with a note saying it is outside the fixture's purpose, so the numbers stay exact and the fixture stays honest about what the engine printed.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `python -m pytest tests/test_labels.py -q`
Expected: `7 passed`.

- [ ] **Step 7: Commit**

```bash
git add bench/labels.py bench/label_tool.py label.py LABELLING.md fixtures/labels tests/test_labels.py
git commit -m "bench: two-pass labels, the label tool and the labelling protocol"
```

---

### Task 6: The scorer

**Files:**
- Create: `bench/score.py`
- Test: `tests/test_score.py`

**Interfaces:**
- Consumes: `bench.run.Finding`, `bench.run.RuleMeta`, `bench.labels.LabelFile`, `bench.labels.Entry`.
- Produces:
  - `NOT_BENCHMARKED: dict[str, str]` = `{"vulnerable-dependency": "advisory feed changes daily", "boundary-violation": "needs per-repository config", "express-route-without-auth": "needs per-repository config"}`.
  - `MIN_N = 5`, `LINE = 0.85`.
  - `Score` dataclass: `key: str` (rule or `rule@language`), `true: int`, `false_positive: int`, `missed: int`, `precision: float | None`, `recall: float | None`, `scored: bool`, `reason: str`.
  - `score(findings: list[Finding], labels: dict[str, LabelFile], rules: dict[str, RuleMeta]) -> tuple[list[Score], list[Score], list[Finding]]`: per-rule scores (one per rule the engine lists, in the SARIF rule order, plus every rule appearing in labels), per-pair scores (`rule@language` for every pair with at least one label or finding), and the list of unlabelled findings (reported by the engine with no entry in any label file), which the run flags so labelling gaps are visible.
  - Matching: a finding matches an entry when `diff`, `rule`, `file` and `line` are equal. Only confirmed entries count. `not-applicable` entries and their findings are dropped from every count. `missed` entries count toward recall only. A finding with no confirmed entry is "unlabelled" and counts nowhere.
  - `render_markdown(per_rule, per_pair, version: str, corpus_size: int, unlabelled: int) -> str`: a table with columns `Rule | Ships | Precision | Recall | True | False positive | Missed | Note`, then the pair table with `Pair` instead of `Rule`. Precision and recall are printed as percentages with no decimals; below the line they are suffixed with ` (below line)`; not scored rows print `n<5, not scored` or the not-benchmarked reason. The ships column says `on`, `off` or `locked` (`secret-exposed`).

- [ ] **Step 1: Write the failing tests**

`tests/test_score.py`:
```python
from bench.labels import Entry, LabelFile
from bench.run import Finding, RuleMeta
from bench.score import Score, render_markdown, score


def F(diff, rule, file, line, lang="typescript"):
    return Finding(diff, rule, file, line, "0" * 16, "high", lang)


def E(rule, file, line, v, ident="0" * 16):
    return Entry(rule=rule, file=file, line=line, id=None if v == "missed" else ident, pass1=v, pass2=v, note="")


RULES = {r: RuleMeta(r, r != "dead-file", ["typescript"]) for r in ["leftover-debug", "unreachable", "dead-file", "vulnerable-dependency"]}


def labels(**files):
    return {d: LabelFile(d, "v0.5.0", {}, {}, es) for d, es in files.items()}


def test_precision_and_recall_per_rule():
    fs = [F("a", "leftover-debug", "x.ts", 1), F("a", "leftover-debug", "x.ts", 2), F("b", "unreachable", "y.ts", 3)]
    ls = labels(a=[E("leftover-debug", "x.ts", 1, "true"), E("leftover-debug", "x.ts", 2, "false-positive")],
                b=[E("unreachable", "y.ts", 3, "true"), E("unreachable", "y.ts", 9, "missed")])
    per_rule, _, unlabelled = score(fs, ls, RULES)
    by = {s.key: s for s in per_rule}
    assert (by["leftover-debug"].true, by["leftover-debug"].false_positive, by["leftover-debug"].missed) == (1, 1, 0)
    assert by["leftover-debug"].precision == 0.5 and by["leftover-debug"].recall == 1.0
    assert by["unreachable"].precision == 1.0 and by["unreachable"].recall == 0.5
    assert unlabelled == []


def test_small_samples_are_not_scored_and_not_benchmarked_rules_say_why():
    fs = [F("a", "leftover-debug", "x.ts", 1)]
    ls = labels(a=[E("leftover-debug", "x.ts", 1, "true")])
    per_rule, _, _ = score(fs, ls, RULES)
    by = {s.key: s for s in per_rule}
    assert by["leftover-debug"].scored is False and by["leftover-debug"].reason == "n<5, not scored"
    assert by["leftover-debug"].precision == 1.0
    assert by["vulnerable-dependency"].scored is False and "advisory" in by["vulnerable-dependency"].reason


def test_unconfirmed_and_not_applicable_entries_are_dropped_and_unlabelled_findings_flagged():
    fs = [F("a", "leftover-debug", "x.ts", 1), F("a", "leftover-debug", "x.ts", 2), F("a", "leftover-debug", "x.ts", 3)]
    ls = labels(a=[Entry("leftover-debug", "x.ts", 1, "0" * 16, "true", "false-positive", ""),
                   E("leftover-debug", "x.ts", 2, "not-applicable")])
    per_rule, _, unlabelled = score(fs, ls, RULES)
    by = {s.key: s for s in per_rule}
    assert (by["leftover-debug"].true, by["leftover-debug"].false_positive) == (0, 0)
    assert [(f.file, f.line) for f in unlabelled] == [("x.ts", 3)]


def test_pairs_split_by_language():
    fs = [F("a", "leftover-debug", "x.ts", 1), F("a", "leftover-debug", "y.php", 2, "php")]
    ls = labels(a=[E("leftover-debug", "x.ts", 1, "true"), E("leftover-debug", "y.php", 2, "false-positive")])
    _, per_pair, _ = score(fs, ls, RULES)
    by = {s.key: s for s in per_pair}
    assert by["leftover-debug@typescript"].precision == 1.0
    assert by["leftover-debug@php"].precision == 0.0


def test_render_marks_below_line_ships_off_and_reasons():
    per_rule = [
        Score("leftover-debug", 17, 3, 0, 0.85, 1.0, True, ""),
        Score("unreachable", 4, 2, 0, 4 / 6, 1.0, True, ""),
        Score("dead-file", 1, 0, 0, 1.0, 1.0, False, "n<5, not scored"),
        Score("vulnerable-dependency", 0, 0, 0, None, None, False, "not benchmarked: advisory feed changes daily"),
    ]
    md = render_markdown(per_rule, [], "v0.5.0", corpus_size=10, unlabelled=0)
    assert "| `leftover-debug` | on | 85% | 100% | 17 | 3 | 0 |" in md
    assert "| `unreachable` | on | 67% (below line) | 100% |" in md
    assert "| `dead-file` | off | 100% | 100% | 1 | 0 | 0 | n<5, not scored |" in md
    assert "not benchmarked: advisory feed changes daily" in md
    assert "Locrin v0.5.0" in md and "10 diffs" in md
```

- [ ] **Step 2: Run them to verify they fail**

Run: `python -m pytest tests/test_score.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.score'`.

- [ ] **Step 3: Write the implementation**

`bench/score.py`:
```python
"""Join findings with confirmed labels and compute precision and recall."""
from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass

from bench.labels import LabelFile
from bench.run import Finding, RuleMeta

NOT_BENCHMARKED = {
    "vulnerable-dependency": "advisory feed changes daily",
    "boundary-violation": "needs per-repository config",
    "express-route-without-auth": "needs per-repository config",
}
LOCKED = {"secret-exposed"}
MIN_N = 5
LINE = 0.85


@dataclass
class Score:
    key: str
    true: int
    false_positive: int
    missed: int
    precision: float | None
    recall: float | None
    scored: bool
    reason: str


def _tally(counts: dict[str, list[int]], keys_in_order: list[str]) -> list[Score]:
    out = []
    for key in keys_in_order:
        t, fp, m = counts.get(key, [0, 0, 0])
        rule = key.split("@", 1)[0]
        precision = t / (t + fp) if t + fp else None
        recall = t / (t + m) if t + m else None
        if rule in NOT_BENCHMARKED:
            out.append(Score(key, t, fp, m, precision, recall, False, f"not benchmarked: {NOT_BENCHMARKED[rule]}"))
        elif t + fp + m < MIN_N:
            out.append(Score(key, t, fp, m, precision, recall, False, "n<5, not scored"))
        else:
            out.append(Score(key, t, fp, m, precision, recall, True, ""))
    return out


def score(findings: list[Finding], labels: dict[str, LabelFile], rules: dict[str, RuleMeta]) -> tuple[list[Score], list[Score], list[Finding]]:
    by_key: dict[tuple[str, str, str, int], str] = {}
    for diff, lf in labels.items():
        for e in lf.entries:
            if e.confirmed:
                by_key[(diff, e.rule, e.file, e.line)] = e.verdict
    rule_counts: dict[str, list[int]] = defaultdict(lambda: [0, 0, 0])
    pair_counts: dict[str, list[int]] = defaultdict(lambda: [0, 0, 0])
    unlabelled: list[Finding] = []
    seen = set()
    for f in findings:
        k = (f.diff, f.rule, f.file, f.line)
        v = by_key.get(k)
        if v is None:
            unlabelled.append(f)
            continue
        seen.add(k)
        if v == "not-applicable":
            continue
        idx = 0 if v == "true" else 1 if v == "false-positive" else None
        if idx is None:
            continue
        rule_counts[f.rule][idx] += 1
        if f.language:
            pair_counts[f"{f.rule}@{f.language}"][idx] += 1
    from bench.corpus import language_of
    for (diff, rule, file, line), v in by_key.items():
        if v == "missed":
            rule_counts[rule][2] += 1
            lang = language_of(file)
            if lang:
                pair_counts[f"{rule}@{lang}"][2] += 1
    rule_order = list(rules.keys()) + sorted(r for r in rule_counts if r not in rules)
    pair_order = sorted(pair_counts)
    return _tally(rule_counts, rule_order), _tally(pair_counts, pair_order), unlabelled


def _pct(v: float | None) -> str:
    return "" if v is None else f"{round(v * 100)}%"


def _cell(v: float | None, is_precision: bool, scored: bool) -> str:
    s = _pct(v)
    if s and is_precision and scored and v is not None and v < LINE:
        s += " (below line)"
    return s


def render_markdown(per_rule: list[Score], per_pair: list[Score], version: str, corpus_size: int, unlabelled: int, rules: dict[str, RuleMeta] | None = None) -> str:
    rules = rules or {}

    def ships(key: str) -> str:
        rule = key.split("@", 1)[0]
        if rule in LOCKED:
            return "locked"
        meta = rules.get(rule)
        if meta is None:
            return "on" if rule not in {"dead-file", "swallowed-error", "injection-sink"} else "off"
        return "on" if meta.enabled_by_default else "off"

    def rows(scores: list[Score], head: str) -> list[str]:
        out = [f"| {head} | Ships | Precision | Recall | True | False positive | Missed | Note |", "| --- | --- | --- | --- | --- | --- | --- | --- |"]
        for s in scores:
            out.append(f"| `{s.key}` | {ships(s.key)} | {_cell(s.precision, True, s.scored)} | {_cell(s.recall, False, s.scored)} | {s.true} | {s.false_positive} | {s.missed} | {s.reason} |")
        return out

    lines = [f"Locrin {version}, {corpus_size} diffs, {unlabelled} unlabelled findings.", ""]
    lines += rows(per_rule, "Rule")
    if per_pair:
        lines += ["", *rows(per_pair, "Pair")]
    return "\n".join(lines) + "\n"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python -m pytest tests/test_score.py -q`
Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add bench/score.py tests/test_score.py
git commit -m "bench: scorer with per-rule and per-pair precision and recall"
```

---

### Task 7: run.sh, results layout, README table, end-to-end on fixtures

**Files:**
- Create: `bench/readme_table.py`, `bench/main.py`, `run.sh`
- Test: `tests/test_readme_table.py`, `tests/test_end_to_end.py`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `python -m bench.main --version v0.5.0 [--corpus corpus] [--labels labels] [--out results] [--work .work] [--cache .cache]`: installs, materialises every diff, runs, scores, and writes `results/<version>/findings.jsonl` (one `Finding` per line, sorted), `results/<version>/table.md` (from `render_markdown`), `results/<version>/run.json` (`{"locrin": version, "diffs": n, "unlabelled": n, "materialise_failures": [...], "run_failures": [...], "started": iso, "finished": iso}`), then updates `README.md` between the markers with `table.md` content. Exit 0 when every diff ran, 1 when any diff failed to materialise or run (those are listed in `run.json` and the rest are still scored).
  - `run.sh <version> [args...]`: `set -euo pipefail`, checks `python3` or `python` is 3.12+, `git` and `bash` present, then `exec python -m bench.main --version "$1" "${@:2}"`.
  - `bench.readme_table.replace_table(readme: str, table: str) -> str`: replaces everything between `<!-- results:start -->` and `<!-- results:end -->` (exclusive), raising `ValueError` if a marker is missing.

- [ ] **Step 1: Write the failing tests**

`tests/test_readme_table.py`:
```python
import pytest

from bench.readme_table import replace_table


def test_replaces_between_markers_only():
    readme = "# t\n\n<!-- results:start -->\nold\n<!-- results:end -->\n\n## after\n"
    out = replace_table(readme, "| a |\n")
    assert out == "# t\n\n<!-- results:start -->\n| a |\n<!-- results:end -->\n\n## after\n"


def test_missing_marker_raises():
    with pytest.raises(ValueError, match="results:start"):
        replace_table("no markers", "x")
```

`tests/test_end_to_end.py` (runs only when Locrin is installed; CI installs it):
```python
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
LOCRIN = shutil.which("locrin")
pytestmark = pytest.mark.skipif(LOCRIN is None, reason="locrin not on PATH")


def test_fixture_corpus_scores_exactly(tmp_path):
    version = subprocess.run([LOCRIN, "--version"], capture_output=True, text=True, check=True).stdout.split()[-1]
    env = dict(os.environ, LOCRIN_BIN=LOCRIN)
    out = tmp_path / "results"
    proc = subprocess.run(
        [sys.executable, "-m", "bench.main", "--version", f"v{version}", "--corpus", "fixtures/corpus",
         "--labels", "fixtures/labels", "--out", str(out), "--work", str(tmp_path / "work"),
         "--cache", str(tmp_path / "cache"), "--readme", str(tmp_path / "README.md")],
        cwd=ROOT, env=env, capture_output=True, text=True,
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    run = json.loads((out / f"v{version}" / "run.json").read_text())
    assert run["diffs"] == 10 and run["run_failures"] == [] and run["materialise_failures"] == []
    table = (out / f"v{version}" / "table.md").read_text()
    assert "| `leftover-debug` | on | 100% | 100% | 3 | 0 | 0 | n<5, not scored |" in table
    assert "| `leftover-agent-marker` | on | 67% | 100% | 2 | 1 | 0 | n<5, not scored |" in table
    assert "| `unreachable` | on | 100% | 50% | 1 | 0 | 1 | n<5, not scored |" in table
    assert "| `leftover-debug@php` | on | 100% | 100% | 1 | 0 | 0 | n<5, not scored |" in table
    assert "| `leftover-debug@python` | on | 100% | 100% | 1 | 0 | 0 | n<5, not scored |" in table
    assert run["unlabelled"] == 0
```

Note for the implementer: `--readme` is an extra option on `bench.main` so the test never touches the real README; it defaults to `README.md`.

- [ ] **Step 2: Run them to verify they fail**

Run: `python -m pytest tests/test_readme_table.py tests/test_end_to_end.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.readme_table'`.

- [ ] **Step 3: Write the implementation**

`bench/readme_table.py`:
```python
START = "<!-- results:start -->"
END = "<!-- results:end -->"


def replace_table(readme: str, table: str) -> str:
    if START not in readme:
        raise ValueError(f"README has no {START} marker")
    if END not in readme:
        raise ValueError(f"README has no {END} marker")
    head, rest = readme.split(START, 1)
    _, tail = rest.split(END, 1)
    body = table if table.endswith("\n") else table + "\n"
    return f"{head}{START}\n{body}{END}{tail}"
```

`bench/main.py`:
```python
"""One run: install, materialise, check, score, write results, update the README."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path

from bench.corpus import load_corpus
from bench.labels import load_labels
from bench.materialise import MaterialiseError, materialise
from bench.readme_table import replace_table
from bench.run import Finding, RunError, install_locrin, normalise, run_check
from bench.score import render_markdown, score


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="bench")
    p.add_argument("--version", required=True, help="Locrin version tag, for example v0.5.0")
    p.add_argument("--corpus", default="corpus")
    p.add_argument("--labels", default="labels")
    p.add_argument("--out", default="results")
    p.add_argument("--work", default=".work")
    p.add_argument("--cache", default=".cache")
    p.add_argument("--readme", default="README.md")
    a = p.parse_args(argv)
    started = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
    locrin = install_locrin(a.version, Path(a.cache))
    diffs = load_corpus(Path(a.corpus))
    labels = load_labels(Path(a.labels))
    findings: list[Finding] = []
    rules = {}
    mat_fail, run_fail = [], []
    for d in diffs:
        try:
            co = materialise(d, Path(a.corpus), Path(a.cache))
        except MaterialiseError as e:
            mat_fail.append(str(e))
            print(f"materialise failed: {e}", file=sys.stderr)
            continue
        try:
            doc = run_check(locrin, co, Path(a.work))
        except RunError as e:
            run_fail.append(str(e))
            print(f"run failed: {e}", file=sys.stderr)
            continue
        fs, rs = normalise(d.id, doc)
        findings.extend(fs)
        rules = rs or rules
        print(f"{d.id}: {len(fs)} findings")
    findings.sort(key=lambda f: (f.diff, f.rule, f.file, f.line, f.id))
    per_rule, per_pair, unlabelled = score(findings, labels, rules)
    out = Path(a.out) / a.version
    out.mkdir(parents=True, exist_ok=True)
    with (out / "findings.jsonl").open("w", encoding="utf-8") as fh:
        for f in findings:
            fh.write(json.dumps(f.__dict__, sort_keys=True) + "\n")
    table = render_markdown(per_rule, per_pair, a.version, corpus_size=len(diffs), unlabelled=len(unlabelled), rules=rules)
    (out / "table.md").write_text(table, encoding="utf-8")
    finished = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
    (out / "run.json").write_text(json.dumps({
        "locrin": a.version, "diffs": len(diffs), "findings": len(findings), "unlabelled": len(unlabelled),
        "materialise_failures": mat_fail, "run_failures": run_fail, "started": started, "finished": finished,
    }, indent=2) + "\n", encoding="utf-8")
    readme = Path(a.readme)
    if readme.exists():
        readme.write_text(replace_table(readme.read_text(encoding="utf-8"), table), encoding="utf-8")
    for f in unlabelled:
        print(f"unlabelled: {f.diff} {f.rule} {f.file}:{f.line}", file=sys.stderr)
    print(table)
    return 1 if (mat_fail or run_fail) else 0


if __name__ == "__main__":
    sys.exit(main())
```

`run.sh`:
```bash
#!/usr/bin/env bash
# Usage: ./run.sh v0.5.0 [extra bench options]
set -euo pipefail
cd "$(dirname "$0")"
[[ $# -ge 1 ]] || { echo "usage: ./run.sh <locrin version tag> [options]" >&2; exit 2; }
py=python3; command -v python3 >/dev/null || py=python
"$py" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 12) else 1)' || { echo "run.sh: Python 3.12 or newer is required" >&2; exit 2; }
command -v git >/dev/null || { echo "run.sh: git is required" >&2; exit 2; }
exec "$py" -m bench.main --version "$1" "${@:2}"
```

Mark it executable in git: `git update-index --chmod=+x run.sh`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python -m pytest -q`
Expected: every test passes, including the end-to-end test, since Locrin 0.5.0 is on this machine's PATH. If the end-to-end numbers differ from the expected lines, the fixture or its label is wrong, not the assertion: go back to the Task 2 and Task 5 files and fix them so that the arithmetic in the table above holds (three true `leftover-debug`, two true and one false `leftover-agent-marker`, one true and one missed `unreachable`).

- [ ] **Step 5: Commit**

```bash
git add bench/readme_table.py bench/main.py run.sh tests/test_readme_table.py tests/test_end_to_end.py
git commit -m "bench: one-command run with results directory and README table"
```

---

### Task 8: The corpus builder

**Files:**
- Create: `bench/build_corpus.py`, `fixtures/github/search.json`, `fixtures/github/repo.json`, `fixtures/github/commit.json`, `fixtures/github/contents_before.json`, `fixtures/github/contents_after.json`
- Test: `tests/test_build_corpus.py`

**Interfaces:**
- Consumes: `bench.corpus` constants, `bench.corpus.language_of`.
- Produces:
  - `GitHub` class with one method `get(path: str, params: dict | None = None, accept: str = "application/vnd.github+json") -> dict`, using `urllib` against `https://api.github.com`, sending `Authorization: Bearer <token>` from the `GITHUB_TOKEN` environment variable, raising `BuildError` on non-2xx. The tests replace it with a fake fed from `fixtures/github/`.
  - `TRAILERS = ["Co-Authored-By: Claude", "Co-authored-by: Codex", "Co-authored-by: Copilot", "Co-authored-by: Cursor"]`.
  - `candidates(gh, trailer: str, per_page: int = 100, pages: int = 3) -> list[dict]`: `GET /search/commits?q="<trailer>" is:public&sort=committer-date&order=desc` with the `application/vnd.github.cloak-preview+json` accept header not required today; use the default JSON accept. Returns the raw items.
  - `accept(gh, item) -> dict | None`: applies the filters and returns a record dict plus `before`/`after` file contents, or `None` with a reason logged:
    1. Not a merge commit (`len(item["parents"]) == 1`).
    2. Repository licence from `GET /repos/{full_name}` `license.spdx_id` in `ALLOWED_LICENCES`; repository not a fork, not archived.
    3. Commit detail from `GET /repos/{full_name}/commits/{sha}`: between 1 and 30 files; at least one file whose `language_of` is not None and whose status is `added` or `modified`; no file over 200 KB.
    4. The corpus language is the language of the majority of supported files (ties broken by the order typescript, javascript, php, python; `tsx` counts as typescript).
    5. At most 3 accepted commits per repository across a build (diversity).
    6. Before and after contents come from `GET /repos/{full_name}/contents/{path}?ref=<parent|sha>` decoded from base64; a file added has no before, a file removed has no after.
  - `write_record(out_root: Path, rec: dict, before: dict[str, bytes], after: dict[str, bytes]) -> Path`: writes `<id>.json` and the files under `<id>/before/` and `<id>/after/`, skipping the record if `<id>.json` already exists.
  - CLI: `python -m bench.build_corpus --out corpus --target 300 [--trailer ...] [--repo owner/name --sha <sha>]`. The `--repo/--sha` form accepts a single named commit, for CONTRIBUTING.
  - Rate limits: the builder sleeps 2 seconds between search pages and honours `Retry-After` on 403 and 429 by sleeping that long once, then raising `BuildError` if it happens again.

- [ ] **Step 1: Capture the API fixtures**

With `gh api`, capture one page of search results, one repository, one commit and two contents responses into `fixtures/github/`. Choose a real MIT or Apache-2.0 repository commit with a Claude trailer that touches one or two TypeScript files; the fixture files are data, not corpus, so licence does not matter beyond the test, but pick a permissive one anyway. Strip nothing; the tests read the fields the builder reads.

```bash
gh api -X GET search/commits -f q='"Co-Authored-By: Claude" is:public' -f per_page=5 > fixtures/github/search.json
```
Then for a chosen item's `repository.full_name` and `sha`:
```bash
gh api repos/<owner>/<name> > fixtures/github/repo.json
gh api repos/<owner>/<name>/commits/<sha> > fixtures/github/commit.json
gh api "repos/<owner>/<name>/contents/<file>?ref=<parent>" > fixtures/github/contents_before.json
gh api "repos/<owner>/<name>/contents/<file>?ref=<sha>" > fixtures/github/contents_after.json
```
Record which item, file and shas were chosen in the task report; the test asserts against them.

- [ ] **Step 2: Write the failing tests**

`tests/test_build_corpus.py`:
```python
import json
from pathlib import Path

import pytest

from bench.build_corpus import BuildError, accept, candidates, write_record
from bench.corpus import load_corpus

FX = Path(__file__).resolve().parent.parent / "fixtures" / "github"


class FakeGitHub:
    def __init__(self, overrides=None):
        self.calls = []
        self.overrides = overrides or {}

    def get(self, path, params=None, accept=None):
        self.calls.append(path)
        if path in self.overrides:
            return self.overrides[path]
        if path.startswith("search/commits"):
            return json.loads((FX / "search.json").read_text())
        if "/contents/" in path:
            ref = (params or {}).get("ref", "")
            commit = json.loads((FX / "commit.json").read_text())
            name = "contents_before.json" if ref == commit["parents"][0]["sha"] else "contents_after.json"
            return json.loads((FX / name).read_text())
        if "/commits/" in path:
            return json.loads((FX / "commit.json").read_text())
        if path.startswith("repos/"):
            return json.loads((FX / "repo.json").read_text())
        raise AssertionError(path)


def first_item():
    return json.loads((FX / "search.json").read_text())["items"][0]


def test_candidates_queries_the_trailer(tmp_path):
    gh = FakeGitHub()
    items = candidates(gh, "Co-Authored-By: Claude", pages=1)
    assert items and gh.calls[0].startswith("search/commits")


def test_accept_builds_a_record_with_before_and_after(tmp_path):
    gh = FakeGitHub()
    rec, before, after = accept(gh, first_item(), seen_repos={})
    assert rec["source"] == "git" and rec["licence"] in {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC"}
    assert rec["id"] == f"{rec['repo'].replace('/', '__')}__{rec['sha'][:7]}"
    assert rec["files"] and all("\\" not in f for f in rec["files"])
    assert set(after) <= set(rec["files"]) and set(before) <= set(rec["files"])
    path = write_record(tmp_path, rec, before, after)
    assert path.name == f"{rec['id']}.json"
    assert load_corpus(tmp_path)[0].id == rec["id"]


def test_accept_rejects_bad_licence_merge_commits_and_repo_cap():
    repo = json.loads((FX / "repo.json").read_text())
    bad = dict(repo, license={"spdx_id": "GPL-3.0"})
    gh = FakeGitHub({f"repos/{repo['full_name']}": bad})
    assert accept(gh, first_item(), seen_repos={}) is None
    item = first_item()
    item["parents"] = item["parents"] + item["parents"]
    assert accept(FakeGitHub(), item, seen_repos={}) is None
    assert accept(FakeGitHub(), first_item(), seen_repos={repo["full_name"]: 3}) is None


def test_write_record_is_idempotent(tmp_path):
    rec, before, after = accept(FakeGitHub(), first_item(), seen_repos={})
    write_record(tmp_path, rec, before, after)
    stamp = (tmp_path / f"{rec['id']}.json").stat().st_mtime_ns
    write_record(tmp_path, rec, before, after)
    assert (tmp_path / f"{rec['id']}.json").stat().st_mtime_ns == stamp
```

- [ ] **Step 3: Run them to verify they fail**

Run: `python -m pytest tests/test_build_corpus.py -q`
Expected: FAIL, `ModuleNotFoundError: No module named 'bench.build_corpus'`.

- [ ] **Step 4: Write the implementation**

`bench/build_corpus.py`:
```python
"""Build corpus records from GitHub commits that carry an agent co-author trailer."""
from __future__ import annotations

import argparse
import base64
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

from bench.corpus import ALLOWED_LICENCES, language_of

API = "https://api.github.com"
TRAILERS = ["Co-Authored-By: Claude", "Co-authored-by: Codex", "Co-authored-by: Copilot", "Co-authored-by: Cursor"]
MAX_FILES = 30
MAX_BYTES = 200_000
PER_REPO = 3
LANG_ORDER = ["typescript", "javascript", "php", "python"]


class BuildError(Exception):
    pass


class GitHub:
    def __init__(self, token: str | None = None):
        self.token = token or os.environ.get("GITHUB_TOKEN", "")
        self._retried = False

    def get(self, path: str, params: dict | None = None, accept: str = "application/vnd.github+json") -> dict:
        url = f"{API}/{path}"
        if params:
            url += "?" + urllib.parse.urlencode(params)
        req = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "locrin-benchmark", **({"Authorization": f"Bearer {self.token}"} if self.token else {})})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code in (403, 429) and not self._retried:
                self._retried = True
                time.sleep(int(e.headers.get("Retry-After", "60")))
                return self.get(path, params, accept)
            raise BuildError(f"GET {path}: HTTP {e.code}") from e


def candidates(gh, trailer: str, per_page: int = 100, pages: int = 3) -> list[dict]:
    items = []
    for page in range(1, pages + 1):
        doc = gh.get("search/commits", {"q": f'"{trailer}" is:public', "sort": "committer-date", "order": "desc", "per_page": per_page, "page": page})
        items.extend(doc.get("items", []))
        if len(doc.get("items", [])) < per_page:
            break
        time.sleep(2)
    return items


def _corpus_language(files: list[dict]) -> str | None:
    langs = Counter()
    for f in files:
        lang = language_of(f["filename"])
        if lang and f["status"] in ("added", "modified"):
            langs["typescript" if lang == "tsx" else lang] += 1
    if not langs:
        return None
    best = max(langs.values())
    return next(l for l in LANG_ORDER if langs.get(l) == best)


def _contents(gh, repo: str, path: str, ref: str) -> bytes | None:
    try:
        doc = gh.get(f"repos/{repo}/contents/{urllib.parse.quote(path)}", {"ref": ref})
    except BuildError:
        return None
    if doc.get("encoding") != "base64":
        return None
    return base64.b64decode(doc["content"])


def accept(gh, item: dict, seen_repos: dict[str, int]):
    repo = item["repository"]["full_name"]
    sha = item["sha"]
    if len(item.get("parents", [])) != 1:
        print(f"skip {repo}@{sha[:7]}: merge or root commit", file=sys.stderr)
        return None
    if seen_repos.get(repo, 0) >= PER_REPO:
        print(f"skip {repo}@{sha[:7]}: repository cap", file=sys.stderr)
        return None
    meta = gh.get(f"repos/{repo}")
    licence = (meta.get("license") or {}).get("spdx_id")
    if licence not in ALLOWED_LICENCES or meta.get("fork") or meta.get("archived"):
        print(f"skip {repo}@{sha[:7]}: licence {licence}, fork={meta.get('fork')}, archived={meta.get('archived')}", file=sys.stderr)
        return None
    commit = gh.get(f"repos/{repo}/commits/{sha}")
    files = commit.get("files", [])
    if not 1 <= len(files) <= MAX_FILES or any(f.get("changes", 0) > MAX_BYTES for f in files):
        print(f"skip {repo}@{sha[:7]}: {len(files)} files", file=sys.stderr)
        return None
    language = _corpus_language(files)
    if language is None:
        print(f"skip {repo}@{sha[:7]}: no supported source file", file=sys.stderr)
        return None
    parent = commit["parents"][0]["sha"]
    supported = [f["filename"] for f in files if language_of(f["filename"])]
    before, after = {}, {}
    for f in files:
        name = f["filename"]
        if not language_of(name):
            continue
        if f["status"] != "added":
            b = _contents(gh, repo, name, parent)
            if b is not None and len(b) <= MAX_BYTES:
                before[name] = b
        if f["status"] != "removed":
            a = _contents(gh, repo, name, sha)
            if a is not None and len(a) <= MAX_BYTES:
                after[name] = a
    rec = {
        "id": f"{repo.replace('/', '__')}__{sha[:7]}",
        "source": "git",
        "repo": repo,
        "sha": sha,
        "parent": parent,
        "licence": licence,
        "language": language,
        "url": f"https://github.com/{repo}/commit/{sha}",
        "files": supported,
    }
    seen_repos[repo] = seen_repos.get(repo, 0) + 1
    return rec, before, after


def write_record(out_root: Path, rec: dict, before: dict[str, bytes], after: dict[str, bytes]) -> Path:
    out_root = Path(out_root)
    out_root.mkdir(parents=True, exist_ok=True)
    path = out_root / f"{rec['id']}.json"
    if path.exists():
        return path
    for sub, files in (("before", before), ("after", after)):
        for name, data in files.items():
            target = out_root / rec["id"] / sub / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
    path.write_text(json.dumps(rec, indent=2) + "\n", encoding="utf-8")
    return path


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="build_corpus")
    p.add_argument("--out", default="corpus")
    p.add_argument("--target", type=int, default=300)
    p.add_argument("--trailer", action="append")
    p.add_argument("--repo")
    p.add_argument("--sha")
    a = p.parse_args(argv)
    gh = GitHub()
    seen: dict[str, int] = {}
    written = 0
    if a.repo and a.sha:
        item = {"sha": a.sha, "repository": {"full_name": a.repo}, "parents": [{}]}
        commit = gh.get(f"repos/{a.repo}/commits/{a.sha}")
        item["parents"] = commit["parents"]
        got = accept(gh, item, seen)
        if got is None:
            return 1
        print(write_record(Path(a.out), *got))
        return 0
    for trailer in a.trailer or TRAILERS:
        for item in candidates(gh, trailer):
            if written >= a.target:
                break
            got = accept(gh, item, seen)
            if got is None:
                continue
            print(write_record(Path(a.out), *got))
            written += 1
    print(f"wrote {written} records", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `python -m pytest tests/test_build_corpus.py -q`
Expected: `4 passed`.

- [ ] **Step 6: Commit**

```bash
git add bench/build_corpus.py fixtures/github tests/test_build_corpus.py
git commit -m "bench: corpus builder from agent co-author commits with licence and size filters"
```

---

### Task 9: CI and the scheduled results job

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/benchmark.yml`

**Interfaces:**
- Consumes: `run.sh`, `bench.main`, the fixture corpus.
- Produces: green CI on every push and pull request; a scheduled job that commits results for a new Locrin release.

- [ ] **Step 1: Write ci.yml**

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: pip install pytest
      - name: Install locrin 0.5.0
        run: |
          curl -fsSL https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh | bash -s v0.5.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - run: locrin --version
      - run: python -m pytest -q
      - name: Fixture run end to end
        run: LOCRIN_BIN="$HOME/.local/bin/locrin" ./run.sh v0.5.0 --corpus fixtures/corpus --labels fixtures/labels --out /tmp/results --readme /tmp/README.md
      - name: No em dashes
        run: |
          if grep -rIl $'\xe2\x80\x94' --exclude-dir=.git --exclude-dir=corpus --exclude-dir=.cache . ; then echo "em dash found" >&2; exit 1; fi
```

- [ ] **Step 2: Write benchmark.yml**

```yaml
name: benchmark
on:
  schedule:
    - cron: "17 3 * * *"
  workflow_dispatch:
    inputs:
      version:
        description: "Locrin version tag to measure (blank: latest release)"
        default: ""
permissions:
  contents: write
jobs:
  measure:
    runs-on: ubuntu-latest
    timeout-minutes: 120
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - name: Resolve version
        id: v
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          v="${{ inputs.version }}"
          if [[ -z "$v" ]]; then v=$(gh release view --repo BilalEjaz/locrin --json tagName --jq .tagName); fi
          echo "version=$v" >> "$GITHUB_OUTPUT"
          if [[ -f "results/$v/run.json" && "${{ github.event_name }}" == "schedule" ]]; then echo "skip=true" >> "$GITHUB_OUTPUT"; fi
      - name: Run
        if: steps.v.outputs.skip != 'true'
        env:
          GITHUB_TOKEN: ${{ github.token }}
        run: ./run.sh "${{ steps.v.outputs.version }}"
      - name: Commit results
        if: steps.v.outputs.skip != 'true'
        run: |
          git config user.name "locrin-benchmark"
          git config user.email "benchmark@locrin.com"
          git add "results/${{ steps.v.outputs.version }}" README.md
          git diff --cached --quiet || git commit -m "results: locrin ${{ steps.v.outputs.version }}"
          git push
```

- [ ] **Step 3: Push a branch and confirm CI is green**

```bash
git add .github/workflows/ci.yml .github/workflows/benchmark.yml
git commit -m "ci: tests on every push; scheduled results for each locrin release"
git push -u origin HEAD
gh run watch --exit-status
```
Expected: the `ci` run succeeds. Run `gh workflow run benchmark.yml -f version=v0.5.0` once on the branch and confirm it commits `results/v0.5.0/` from the empty real corpus (ten fixture diffs are not the corpus; the real corpus is empty until Task 10, so the table is empty and says `0 diffs`). That commit is reverted before merge so Task 11's real run is the first results commit.

- [ ] **Step 4: Commit and open the pull request for Tasks 1 to 9**

Everything so far is one branch, `bench/harness`, one pull request titled "Benchmark harness, fixtures, labelling tool, corpus builder, CI". The lead merges it on the founder's word before Task 10 begins.

---

### Task 10: Corpus wave one

**Files:**
- Create: `corpus/<id>.json` and files for every accepted diff

**Interfaces:**
- Consumes: `bench.build_corpus` CLI.
- Produces: at least 150 accepted records across the four trailers, with the language mix reported in the task report (target at least 90 typescript or javascript, at least 25 php, at least 25 python; if the search cannot reach the php and python floors, the report says so and the lead decides whether to widen the search terms).

- [ ] **Step 1: Build**

```bash
GITHUB_TOKEN=$(gh auth token) python -m bench.build_corpus --out corpus --target 300
```
Then count per language and per licence:
```bash
python - <<'EOF'
import json, collections, pathlib
recs=[json.loads(p.read_text()) for p in pathlib.Path("corpus").glob("*.json")]
print(len(recs), collections.Counter(r["language"] for r in recs), collections.Counter(r["licence"] for r in recs))
EOF
```

- [ ] **Step 2: Dry-run the harness on the corpus without labels**

```bash
./run.sh v0.5.0 --labels /tmp/empty-labels --out /tmp/dry --readme /tmp/README.md
```
(`mkdir -p /tmp/empty-labels` first.) Expected: exit 0, every diff materialises and runs, `run.json` lists no failures. Any diff that fails to materialise or run is removed from `corpus/` and named in the task report with the error; a repository that vanished or a commit the clone cannot reach is the usual cause.

- [ ] **Step 3: Check the private-reference rule on the corpus**

The corpus is other people's public code and may legitimately contain e-mail addresses; that is not a leak. But the records must not contain anything from this machine. Run the engine repository's `scripts/check-private-refs.sh` patterns over `corpus/` (copy the `patterns` array from that script into a one-off grep); it must print nothing.

- [ ] **Step 4: Commit**

```bash
git add corpus
git commit -m "corpus: wave one, <n> diffs from public repositories with agent co-author trailers"
```
One pull request, "Corpus wave one", merged on the founder's word. The corpus directory may be several megabytes; that is expected.

---

### Task 11: Labelling wave one and the first results

**Files:**
- Create: `labels/<id>.json` for every labelled diff; `results/v0.5.0/`; README table

**Interfaces:**
- Consumes: `label.py`, LABELLING.md, the corpus.
- Produces: at least 100 diffs with every entry confirmed; per-rule sample sizes reported; the first results commit.

- [ ] **Step 1: Pass one**

An Opus implementer subagent, given LABELLING.md and a list of diff ids, runs `python label.py new <id> --locrin v0.5.0 --by opus` for each, reads the diff (`git -C .cache/repos/<owner>__<name> show <sha>`) and the code around each finding, fills `pass1` on every entry, and adds `missed` entries. The brief tells it: do not look at engine severities or confidence when judging; judge against the definitions only; write a one-line `note` for every `false-positive` and `missed`. Batches of 25 diffs per subagent.

- [ ] **Step 2: Pass two**

A Fable reviewer subagent, given the same guide and ids, opens each label file with `pass1` hidden (the lead strips it into a side file before dispatch and restores it after), fills `pass2`, and adds any `missed` entries pass one lacked (those get `pass1: "?"` and go back to pass one). Then `python label.py confirm <id> --by fable`.

- [ ] **Step 3: Settle disagreements**

`python label.py status` lists them. The lead reads each disputed entry and settles it by editing both passes with a note. Entries the lead cannot settle stay disagreeing and are excluded; the report lists them.

- [ ] **Step 4: Run and commit results**

```bash
./run.sh v0.5.0
git add labels results/v0.5.0 README.md
git commit -m "results: locrin v0.5.0 on <n> labelled diffs"
```
The task report includes the per-rule table and names every rule below the line and every rule with `n<5`. One pull request, "Labels wave one and results for v0.5.0", merged on the founder's word.

- [ ] **Step 5: Link from the engine README**

The engine README already links to the benchmark repository in two places (the Benchmark section and the Corpora section), so no engine change is needed. If the founder wants the headline numbers in the engine README, that is a separate one-line change to make after the numbers exist, not before.

---

## After-plan checklist for the lead

- The benchmark repository is public from the first push; it holds no engine code and no private data, so there is no flip.
- `benchmark.yml` needs no secret: `github.token` has `contents: write` on its own repository through the `permissions` block. Confirm under the repository's Actions settings that workflow permissions allow write, or the commit step fails with a 403 and the fix is that setting.
- Corpus files carry third-party licences; the README says so and each record names the licence. Nothing else is required by MIT, Apache-2.0, BSD or ISC for redistribution of source excerpts beyond keeping the notice, which the record provides through the licence id and source URL.
- Deferred, not in this plan: head-to-head vendor comparison (spec 5.4), a web page for the results (the README table is the page), adding Rust or Go once the engine reads them.

## Self-review against spec section 5

- 5.1 corpus: Task 8 (selection, licences, before and after files), Task 10 (wave one). Labels one record per diff with per-rule expected findings and two-pass confirmation: Task 5, Task 11. No private code: Task 8 samples only public repositories; Task 10 step 3 checks the machine's own paths never enter.
- 5.2 harness: `run.sh` plus Python scorer, installs a named version via the curl installer (Task 4), runs check over every diff (Task 7), matches by rule id, file and anchor line (Task 6), prints precision and recall per rule and per pair (Task 6). Deterministic: offline runs, own cache dir, sorted output, advisory rule excluded (ruling 3). Fixture corpus of ten with an exact-number test: Tasks 2, 5, 7.
- 5.3 results: README table regenerated by CI on each release and committed with the version (Task 9, ruling 5). Below-line marking and ships-off status: Task 6 render. Public corpus does not gate engine releases: nothing here touches the engine's CI.
- 5.4: no vendor tool is run anywhere in this plan.
- Section 6 testing: install test through `install_locrin` in CI (Task 9), scorer fixture test (Task 7).
- Type consistency check: `Finding(diff, rule, file, line, id, confidence, language)` is used with those names in Tasks 4, 5, 6, 7; `Checkout(root, base_ref, removed)` in Tasks 3, 4, 7; `LabelFile.entries[].confirmed / verdict` in Tasks 5, 6; `RuleMeta(id, enabled_by_default, languages)` in Tasks 4, 6, 7; `render_markdown(per_rule, per_pair, version, corpus_size, unlabelled, rules=None)` in Tasks 6 and 7.
- Placeholder scan: the `if False else` line in Task 3 is called out as not to be committed; no TBD or "add validation" phrasing remains.
