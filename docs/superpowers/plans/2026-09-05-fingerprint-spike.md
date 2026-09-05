# Fingerprinting Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure, on the founder's real repositories, whether the three-signal fingerprinting approach from the spec (structural hash, MinHash near-duplicate, signature vector) finds near-duplicate TypeScript functions at 85 percent precision or better at 90 percent recall, and report one number plus the thresholds that produced it.

**Architecture:** Throwaway Python package `spike/fingerprint/fp` with one module per signal, a repo indexer that emits scored candidate pairs, a mutation generator that plants known duplicates for recall, a labelling CLI for precision, and an evaluator that sweeps thresholds and writes `REPORT.md`. Nothing here ships. The production engine is Rust and is planned separately after this reports.

**Tech Stack:** Python 3.12, tree-sitter 0.23 with tree-sitter-typescript, datasketch (MinHash and LSH), pytest. Windows host, Git Bash shell, virtualenv at `spike/fingerprint/.venv`.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, sections 3.3 (fingerprinting) and 10.4 (the spike).

## Global Constraints

- Time box: two weeks maximum from first commit. Whatever the number is on day fourteen is the number.
- Code is throwaway: no packaging, no CLI polish, no docs beyond `REPORT.md` and the README.
- Semantic (Type-4) clones are out of scope; do not add signals to chase them.
- Precision target 85 percent at recall 90 percent, both defined in Task 9. The report states the number even if it fails.
- Minimum function size 40 normalised tokens; smaller functions are ignored everywhere (extractor, planting, labelling).
- No LLM anywhere in the pipeline. Labelling is done by a person (Claude may pre-label with a written reason; the founder spot-checks 50 pairs, see Task 10).
- Repos under test, read-only: `<home>/fasting-app` (folders `src`, `app`, `components`), `<home>/strongspan/src`, `<home>/food-data-platform` (folders `apps`, `packages`). Exclude any `node_modules`, `android`, `ios`, `.expo`, `dist`, `build`, `coverage`, `__mocks__`, and `*.d.ts`.
- Git: branch `spike/fingerprint` off `main`, one commit per task, plain commit messages, no attribution trailers, pull request at the end. Never `git add -A`.
- Every Python command runs through the venv interpreter: `spike/fingerprint/.venv/Scripts/python`. Written below as `$PY` for brevity; executors substitute the full path or export `PY` in their shell.

## File structure

```
spike/fingerprint/
  README.md                 what this is, how to run, where the number lives
  requirements.txt          pinned deps
  pytest.ini                testpaths
  fp/__init__.py
  fp/parse.py               tree-sitter parser setup, parse a file to a tree
  fp/extract.py             find function-like nodes, produce FunctionRecord
  fp/normalize.py           normalised token stream and structural hash
  fp/minhash.py             shingles, MinHash, LSH index wrapper
  fp/signature.py           signature vector and similarity
  fp/index.py               walk repos, build records, emit candidate pairs
  fp/mutate.py              planted near-duplicates for recall
  fp/label.py               terminal labelling loop, writes labels.jsonl
  fp/evaluate.py            threshold sweep, precision at recall, REPORT.md
  tests/fixtures/           small .ts files used by tests
  tests/test_extract.py
  tests/test_normalize.py
  tests/test_minhash.py
  tests/test_signature.py
  tests/test_index.py
  tests/test_mutate.py
  tests/test_evaluate.py
  data/                     gitignored: candidates.jsonl, planted.jsonl, labels.jsonl
  REPORT.md                 the one number, thresholds, and decision
```

Shared record types, defined once in `fp/extract.py` and imported everywhere:

```python
@dataclass(frozen=True)
class FunctionRecord:
    id: str            # f"{file}:{start_line}:{name}"
    file: str          # absolute path, forward slashes
    name: str          # declared name or "<anon>"
    start_line: int    # 1-based
    end_line: int
    source: str        # exact source text of the function node
```

---

### Task 1: Scaffold the spike package and branch

**Files:**
- Create: `spike/fingerprint/README.md`
- Create: `spike/fingerprint/requirements.txt`
- Create: `spike/fingerprint/pytest.ini`
- Create: `spike/fingerprint/fp/__init__.py`
- Create: `spike/fingerprint/tests/test_smoke.py`
- Modify: `.gitignore`

**Interfaces:**
- Produces: a working venv at `spike/fingerprint/.venv` and a passing pytest run that later tasks extend.

- [ ] **Step 1: Create the branch**

```bash
cd <repo> && git checkout -b spike/fingerprint
```

- [ ] **Step 2: Write requirements and pytest config**

`spike/fingerprint/requirements.txt`:
```
tree-sitter==0.23.2
tree-sitter-typescript==0.23.2
datasketch==1.6.5
pytest==8.3.3
```

`spike/fingerprint/pytest.ini`:
```
[pytest]
testpaths = tests
```

`spike/fingerprint/fp/__init__.py`: empty file.

- [ ] **Step 3: Write the smoke test**

`spike/fingerprint/tests/test_smoke.py`:
```python
def test_imports():
    import tree_sitter
    import tree_sitter_typescript
    import datasketch
    assert tree_sitter and tree_sitter_typescript and datasketch
```

- [ ] **Step 4: Create the venv and install**

```bash
cd <repo>/spike/fingerprint && python -m venv .venv && .venv/Scripts/python -m pip install -q -r requirements.txt
```
Expected: no errors. If `tree-sitter-typescript` fails to find a wheel, pin `tree-sitter==0.22.3` and `tree-sitter-typescript==0.21.2` and re-run; note the change in README.

- [ ] **Step 5: Run the smoke test**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest -q
```
Expected: `1 passed`.

- [ ] **Step 6: Gitignore and README**

Append to `<repo>/.gitignore`:
```
spike/fingerprint/.venv/
spike/fingerprint/data/
__pycache__/
.pytest_cache/
```

`spike/fingerprint/README.md`:
```markdown
# Fingerprinting spike (throwaway)

Measures whether structural hash + MinHash + signature vector finds near-duplicate
TypeScript functions at >= 85% precision at 90% recall on the founder's repos.

Run: `.venv/Scripts/python -m pytest` for tests. Pipeline commands are in
`docs/superpowers/plans/2026-09-05-fingerprint-spike.md`. The result lives in `REPORT.md`.

Nothing in here ships. The production engine is Rust.
```

- [ ] **Step 7: Commit**

```bash
cd <repo> && git add .gitignore spike/fingerprint/README.md spike/fingerprint/requirements.txt spike/fingerprint/pytest.ini spike/fingerprint/fp/__init__.py spike/fingerprint/tests/test_smoke.py && git commit -m "spike: scaffold fingerprint spike package"
```

---

### Task 2: Parser setup and function extraction

**Files:**
- Create: `spike/fingerprint/fp/parse.py`
- Create: `spike/fingerprint/fp/extract.py`
- Create: `spike/fingerprint/tests/fixtures/basic.ts`
- Create: `spike/fingerprint/tests/test_extract.py`

**Interfaces:**
- Produces: `parse.parse_source(source: str, tsx: bool) -> tree_sitter.Tree`, `parse.parse_file(path: str) -> tree_sitter.Tree`, `extract.FunctionRecord` (dataclass above), `extract.extract_functions(path: str, source: str) -> list[FunctionRecord]`, `extract.function_nodes(tree) -> list[tree_sitter.Node]`.

- [ ] **Step 1: Write the fixture**

`spike/fingerprint/tests/fixtures/basic.ts`:
```ts
export function add(a: number, b: number): number {
  const total = a + b;
  console.log("adding", total);
  return total;
}

export const multiply = (a: number, b: number): number => {
  const product = a * b;
  console.log("multiplying", product);
  return product;
};

class Calc {
  divide(a: number, b: number): number {
    if (b === 0) {
      throw new Error("div by zero");
    }
    return a / b;
  }
}

const tiny = () => 1;
```

- [ ] **Step 2: Write the failing tests**

`spike/fingerprint/tests/test_extract.py`:
```python
from pathlib import Path
from fp.extract import extract_functions

FIX = Path(__file__).parent / "fixtures" / "basic.ts"


def records():
    return extract_functions(str(FIX), FIX.read_text(encoding="utf8"))


def test_finds_declaration_arrow_and_method():
    names = {r.name for r in records()}
    assert {"add", "multiply", "divide"} <= names


def test_records_have_lines_and_source():
    add = next(r for r in records() if r.name == "add")
    assert add.start_line == 1
    assert add.end_line == 5
    assert add.source.startswith("function add(")
    assert add.id == f"{add.file}:1:add"


def test_tiny_arrow_is_still_extracted_here():
    # size filtering happens in normalize/index, not in extract
    assert any(r.name == "tiny" for r in records())
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_extract.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.extract'`.

- [ ] **Step 4: Implement parse.py**

`spike/fingerprint/fp/parse.py`:
```python
from pathlib import Path

import tree_sitter_typescript as tsts
from tree_sitter import Language, Parser, Tree

TS_LANGUAGE = Language(tsts.language_typescript())
TSX_LANGUAGE = Language(tsts.language_tsx())


def parse_source(source: str, tsx: bool = False) -> Tree:
    parser = Parser(TSX_LANGUAGE if tsx else TS_LANGUAGE)
    return parser.parse(source.encode("utf8"))


def parse_file(path: str) -> Tree:
    p = Path(path)
    return parse_source(p.read_text(encoding="utf8", errors="replace"), tsx=p.suffix == ".tsx")
```

- [ ] **Step 5: Implement extract.py**

`spike/fingerprint/fp/extract.py`:
```python
from dataclasses import dataclass
from pathlib import Path

from tree_sitter import Node, Tree

from fp.parse import parse_source

FUNCTION_TYPES = {
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
    "function",  # older grammar name for function_expression
}


@dataclass(frozen=True)
class FunctionRecord:
    id: str
    file: str
    name: str
    start_line: int
    end_line: int
    source: str


def function_nodes(tree: Tree) -> list[Node]:
    found: list[Node] = []
    stack = [tree.root_node]
    while stack:
        node = stack.pop()
        if node.type in FUNCTION_TYPES:
            found.append(node)
        stack.extend(reversed(node.children))
    found.sort(key=lambda n: n.start_byte)
    return found


def _name_of(node: Node) -> str:
    name_node = node.child_by_field_name("name")
    if name_node is not None:
        return name_node.text.decode("utf8")
    parent = node.parent
    if parent is not None and parent.type == "variable_declarator":
        n = parent.child_by_field_name("name")
        if n is not None:
            return n.text.decode("utf8")
    if parent is not None and parent.type == "pair":
        k = parent.child_by_field_name("key")
        if k is not None:
            return k.text.decode("utf8")
    return "<anon>"


def extract_functions(path: str, source: str) -> list[FunctionRecord]:
    file = str(Path(path).resolve()).replace("\\", "/")
    tree = parse_source(source, tsx=file.endswith(".tsx"))
    src_bytes = source.encode("utf8")
    out: list[FunctionRecord] = []
    for node in function_nodes(tree):
        start_line = node.start_point[0] + 1
        end_line = node.end_point[0] + 1
        name = _name_of(node)
        text = src_bytes[node.start_byte:node.end_byte].decode("utf8", errors="replace")
        out.append(FunctionRecord(
            id=f"{file}:{start_line}:{name}",
            file=file,
            name=name,
            start_line=start_line,
            end_line=end_line,
            source=text,
        ))
    return out
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_extract.py -q
```
Expected: `3 passed`. If `function_expression` versus `function` naming differs in the installed grammar, both are in `FUNCTION_TYPES`, so no change is needed.

- [ ] **Step 7: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/parse.py spike/fingerprint/fp/extract.py spike/fingerprint/tests/fixtures/basic.ts spike/fingerprint/tests/test_extract.py && git commit -m "spike: tree-sitter parse and function extraction"
```

---

### Task 3: Normalised token stream and structural hash

**Files:**
- Create: `spike/fingerprint/fp/normalize.py`
- Create: `spike/fingerprint/tests/test_normalize.py`

**Interfaces:**
- Consumes: `fp.parse.parse_source`.
- Produces: `normalize.tokens(source: str) -> list[str]` (normalised token stream for a function's source), `normalize.structural_hash(tokens: list[str]) -> str` (hex sha256), `normalize.MIN_TOKENS = 40`.

- [ ] **Step 1: Write the failing tests**

`spike/fingerprint/tests/test_normalize.py`:
```python
from fp.normalize import MIN_TOKENS, structural_hash, tokens

A = """function add(a: number, b: number): number {
  const total = a + b;
  console.log("adding", total);
  return total;
}"""

A_RENAMED = """function sum(x: number, y: number): number {
  const result = x + y;
  console.log("summing", result);
  return result;
}"""

B = """function add(a: number, b: number): number {
  const total = a + b;
  if (total > 10) { return 10; }
  return total;
}"""


def test_identifiers_and_literals_are_placeholders():
    t = tokens(A)
    assert "ID" in t and "LIT" in t
    assert "add" not in t and "total" not in t and '"adding"' not in t


def test_renamed_copy_has_same_structural_hash():
    assert structural_hash(tokens(A)) == structural_hash(tokens(A_RENAMED))


def test_changed_body_has_different_hash():
    assert structural_hash(tokens(A)) != structural_hash(tokens(B))


def test_min_tokens_constant():
    assert MIN_TOKENS == 40
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_normalize.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.normalize'`.

- [ ] **Step 3: Implement normalize.py**

`spike/fingerprint/fp/normalize.py`:
```python
import hashlib

from tree_sitter import Node

from fp.parse import parse_source

MIN_TOKENS = 40

IDENTIFIER_TYPES = {
    "identifier",
    "property_identifier",
    "shorthand_property_identifier",
    "shorthand_property_identifier_pattern",
    "type_identifier",
    "private_property_identifier",
    "statement_identifier",
}
LITERAL_TYPES = {
    "string",
    "string_fragment",
    "template_string",
    "number",
    "true",
    "false",
    "null",
    "undefined",
    "regex",
}
SKIP_TYPES = {"comment", "'", '"', "`"}


def _walk(node: Node, out: list[str]) -> None:
    if node.type in SKIP_TYPES:
        return
    if node.type in LITERAL_TYPES:
        out.append("LIT")
        return
    if node.type in IDENTIFIER_TYPES:
        out.append("ID")
        return
    if node.child_count == 0:
        out.append(node.type)
        return
    out.append(node.type)
    for child in node.children:
        _walk(child, out)


def tokens(source: str) -> list[str]:
    tree = parse_source(source)
    out: list[str] = []
    _walk(tree.root_node, out)
    # drop the program wrapper so equal bodies compare equal regardless of file context
    return [t for t in out if t != "program"]


def structural_hash(toks: list[str]) -> str:
    return hashlib.sha256("\x1f".join(toks).encode("utf8")).hexdigest()
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_normalize.py -q
```
Expected: `4 passed`. If the renamed test fails, print `tokens(A)` and `tokens(A_RENAMED)` side by side; any differing token is an identifier or literal node type missing from the sets above. Add it and re-run.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/normalize.py spike/fingerprint/tests/test_normalize.py && git commit -m "spike: normalised token stream and structural hash"
```

---

### Task 4: Shingles, MinHash, and LSH index

**Files:**
- Create: `spike/fingerprint/fp/minhash.py`
- Create: `spike/fingerprint/tests/test_minhash.py`

**Interfaces:**
- Consumes: `fp.normalize.tokens`.
- Produces: `minhash.SHINGLE_K = 5`, `minhash.NUM_PERM = 128`, `minhash.shingles(toks: list[str], k: int = SHINGLE_K) -> set[str]`, `minhash.minhash_of(toks: list[str]) -> datasketch.MinHash`, `minhash.estimated_jaccard(a: MinHash, b: MinHash) -> float`, `minhash.LshIndex` with `.add(key: str, m: MinHash)` and `.query(m: MinHash) -> list[str]`, constructed with `LshIndex(threshold: float)`.

- [ ] **Step 1: Write the failing tests**

`spike/fingerprint/tests/test_minhash.py`:
```python
from fp.minhash import LshIndex, estimated_jaccard, minhash_of, shingles
from fp.normalize import tokens

BASE = """function load(id: string) {
  const res = await fetch(`/api/items/${id}`);
  if (!res.ok) { throw new Error("failed"); }
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

NEAR = """function loadItem(itemId: string) {
  const res = await fetch(`/api/items/${itemId}`);
  if (!res.ok) { throw new Error("load failed"); }
  const data = await res.json();
  cache.set(itemId, data);
  console.log("loaded", itemId);
  return data;
}"""

FAR = """function formatDate(d: Date) {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}"""


def test_shingles_are_k_grams():
    s = shingles(["a", "b", "c", "d", "e", "f"], k=5)
    assert s == {"a b c d e", "b c d e f"}


def test_near_copy_is_similar_and_far_is_not():
    mb, mn, mf = (minhash_of(tokens(x)) for x in (BASE, NEAR, FAR))
    assert estimated_jaccard(mb, mn) > 0.6
    assert estimated_jaccard(mb, mf) < 0.3


def test_lsh_retrieves_near_not_far():
    idx = LshIndex(threshold=0.5)
    idx.add("near", minhash_of(tokens(NEAR)))
    idx.add("far", minhash_of(tokens(FAR)))
    hits = idx.query(minhash_of(tokens(BASE)))
    assert "near" in hits and "far" not in hits
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_minhash.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.minhash'`.

- [ ] **Step 3: Implement minhash.py**

`spike/fingerprint/fp/minhash.py`:
```python
from datasketch import MinHash, MinHashLSH

SHINGLE_K = 5
NUM_PERM = 128


def shingles(toks: list[str], k: int = SHINGLE_K) -> set[str]:
    if len(toks) < k:
        return {" ".join(toks)} if toks else set()
    return {" ".join(toks[i:i + k]) for i in range(len(toks) - k + 1)}


def minhash_of(toks: list[str]) -> MinHash:
    m = MinHash(num_perm=NUM_PERM)
    for s in shingles(toks):
        m.update(s.encode("utf8"))
    return m


def estimated_jaccard(a: MinHash, b: MinHash) -> float:
    return float(a.jaccard(b))


class LshIndex:
    def __init__(self, threshold: float):
        self._lsh = MinHashLSH(threshold=threshold, num_perm=NUM_PERM)

    def add(self, key: str, m: MinHash) -> None:
        self._lsh.insert(key, m)

    def query(self, m: MinHash) -> list[str]:
        return list(self._lsh.query(m))
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_minhash.py -q
```
Expected: `3 passed`. If the similarity assertion is off by a little, adjust the test bounds to 0.5 and 0.35 once, note it in the commit message, and move on; the real thresholds come from the sweep in Task 9.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/minhash.py spike/fingerprint/tests/test_minhash.py && git commit -m "spike: shingles, MinHash and LSH index"
```

---

### Task 5: Signature vector and similarity

**Files:**
- Create: `spike/fingerprint/fp/signature.py`
- Create: `spike/fingerprint/tests/test_signature.py`

**Interfaces:**
- Consumes: `fp.parse.parse_source`.
- Produces: `signature.Signature` dataclass with `param_count: int`, `callees: frozenset[str]`, `returns_value: bool`, `is_async: bool`; `signature.signature_of(source: str) -> Signature`; `signature.similarity(a: Signature, b: Signature) -> float` in 0 to 1; `signature.gate(a, b) -> bool` (True when param counts match or callee Jaccard is at least 0.5).

- [ ] **Step 1: Write the failing tests**

`spike/fingerprint/tests/test_signature.py`:
```python
from fp.signature import gate, signature_of, similarity

A = """async function load(id: string) {
  const res = await fetch(`/api/${id}`);
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

A2 = """async function fetchOne(key: string) {
  const r = await fetch(`/v2/${key}`);
  const body = await r.json();
  cache.set(key, body);
  return body;
}"""

B = """function fmt(d: Date, locale: string) {
  return d.toLocaleDateString(locale);
}"""


def test_signature_fields():
    s = signature_of(A)
    assert s.param_count == 1
    assert s.is_async is True
    assert s.returns_value is True
    assert {"fetch", "json", "set"} <= set(s.callees)


def test_similar_signatures_score_high_and_gate_passes():
    assert similarity(signature_of(A), signature_of(A2)) > 0.8
    assert gate(signature_of(A), signature_of(A2)) is True


def test_different_signatures_score_low_and_gate_fails():
    assert similarity(signature_of(A), signature_of(B)) < 0.4
    assert gate(signature_of(A), signature_of(B)) is False
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_signature.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.signature'`.

- [ ] **Step 3: Implement signature.py**

`spike/fingerprint/fp/signature.py`:
```python
from dataclasses import dataclass

from tree_sitter import Node

from fp.parse import parse_source


@dataclass(frozen=True)
class Signature:
    param_count: int
    callees: frozenset[str]
    returns_value: bool
    is_async: bool


def _first_function(root: Node) -> Node | None:
    stack = [root]
    while stack:
        n = stack.pop()
        if n.type in {"function_declaration", "generator_function_declaration", "method_definition",
                      "arrow_function", "function_expression", "function"}:
            return n
        stack.extend(reversed(n.children))
    return None


def _callee_name(call: Node) -> str | None:
    fn = call.child_by_field_name("function") or call.child_by_field_name("constructor")
    if fn is None:
        return None
    if fn.type == "member_expression":
        prop = fn.child_by_field_name("property")
        return prop.text.decode("utf8") if prop is not None else None
    if fn.type in {"identifier", "type_identifier"}:
        return fn.text.decode("utf8")
    return None


def signature_of(source: str) -> Signature:
    root = parse_source(source).root_node
    fn = _first_function(root) or root
    params = fn.child_by_field_name("parameters")
    param_count = 0
    if params is not None:
        param_count = sum(1 for c in params.children if c.is_named and c.type != "comment")
    elif fn.type == "arrow_function":
        single = fn.child_by_field_name("parameter")
        param_count = 1 if single is not None else 0

    callees: set[str] = set()
    returns_value = False
    is_async = any(c.type == "async" for c in fn.children)
    stack = list(fn.children)
    while stack:
        n = stack.pop()
        if n.type in {"call_expression", "new_expression"}:
            name = _callee_name(n)
            if name:
                callees.add(name)
        if n.type == "return_statement" and any(c.is_named for c in n.children):
            returns_value = True
        stack.extend(n.children)
    if fn.type == "arrow_function":
        body = fn.child_by_field_name("body")
        if body is not None and body.type != "statement_block":
            returns_value = True
    return Signature(param_count, frozenset(callees), returns_value, is_async)


def _jaccard(a: frozenset[str], b: frozenset[str]) -> float:
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def similarity(a: Signature, b: Signature) -> float:
    callee_score = _jaccard(a.callees, b.callees)
    param_score = 1.0 if a.param_count == b.param_count else 0.0
    ret_score = 1.0 if a.returns_value == b.returns_value else 0.0
    async_score = 1.0 if a.is_async == b.is_async else 0.0
    return 0.6 * callee_score + 0.2 * param_score + 0.1 * ret_score + 0.1 * async_score


def gate(a: Signature, b: Signature) -> bool:
    return a.param_count == b.param_count or _jaccard(a.callees, b.callees) >= 0.5
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_signature.py -q
```
Expected: `3 passed`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/signature.py spike/fingerprint/tests/test_signature.py && git commit -m "spike: signature vector and similarity"
```

---

### Task 6: Repo indexer and candidate pair emission

**Files:**
- Create: `spike/fingerprint/fp/index.py`
- Create: `spike/fingerprint/tests/fixtures/repo/a.ts`
- Create: `spike/fingerprint/tests/fixtures/repo/b.ts`
- Create: `spike/fingerprint/tests/fixtures/repo/node_modules/x.ts`
- Create: `spike/fingerprint/tests/test_index.py`

**Interfaces:**
- Consumes: `extract.extract_functions`, `normalize.tokens`, `normalize.structural_hash`, `normalize.MIN_TOKENS`, `minhash.minhash_of`, `minhash.estimated_jaccard`, `minhash.LshIndex`, `signature.signature_of`, `signature.similarity`, `signature.gate`.
- Produces: `index.EXCLUDE_DIRS`, `index.iter_source_files(roots: list[str]) -> Iterator[str]`, `index.Indexed` dataclass (`record: FunctionRecord`, `toks: list[str]`, `shash: str`, `mh: MinHash`, `sig: Signature`), `index.build(roots: list[str]) -> list[Indexed]`, `index.CandidatePair` dataclass (`a_id, b_id, a_file, b_file, a_name, b_name, structural_match: bool, jaccard: float, sig_sim: float, sig_gate: bool, a_source, b_source`), `index.candidate_pairs(items: list[Indexed], floor: float = 0.3) -> list[CandidatePair]`, `index.write_jsonl(pairs, path)`, `index.read_jsonl(path) -> list[CandidatePair]`, and a `__main__` that takes roots and an output path.

- [ ] **Step 1: Write fixtures**

`spike/fingerprint/tests/fixtures/repo/a.ts`:
```ts
export async function loadUser(id: string) {
  const res = await fetch(`/api/users/${id}`);
  if (!res.ok) {
    throw new Error("user load failed");
  }
  const data = await res.json();
  cache.set(id, data);
  return data;
}

export function unrelated(list: number[]) {
  let sum = 0;
  for (const n of list) {
    if (n > 0) {
      sum += n;
    }
  }
  return sum / Math.max(list.length, 1);
}

export const tinyHelper = (n: number) => n + 1;
```

`spike/fingerprint/tests/fixtures/repo/b.ts`:
```ts
export async function loadProfile(userId: string) {
  const res = await fetch(`/api/users/${userId}`);
  if (!res.ok) {
    throw new Error("profile load failed");
  }
  const data = await res.json();
  cache.set(userId, data);
  return data;
}
```

`spike/fingerprint/tests/fixtures/repo/node_modules/x.ts`:
```ts
export function shouldBeIgnored() { return 1; }
```

- [ ] **Step 2: Write the failing tests**

`spike/fingerprint/tests/test_index.py`:
```python
from pathlib import Path

from fp.index import build, candidate_pairs, iter_source_files, read_jsonl, write_jsonl

ROOT = str(Path(__file__).parent / "fixtures" / "repo")


def test_iter_source_files_skips_node_modules():
    files = [Path(f).name for f in iter_source_files([ROOT])]
    assert set(files) == {"a.ts", "b.ts"}


def test_build_filters_small_functions():
    items = build([ROOT])
    names = {i.record.name for i in items}
    assert "loadUser" in names and "loadProfile" in names and "unrelated" in names
    assert "tinyHelper" not in names  # under MIN_TOKENS, dropped by build()


def test_candidate_pairs_find_the_near_duplicate(tmp_path):
    items = build([ROOT])
    pairs = candidate_pairs(items, floor=0.3)
    names = {frozenset((p.a_name, p.b_name)) for p in pairs}
    assert frozenset(("loadUser", "loadProfile")) in names
    hit = next(p for p in pairs if {p.a_name, p.b_name} == {"loadUser", "loadProfile"})
    assert hit.jaccard > 0.5 and hit.sig_gate is True
    out = tmp_path / "c.jsonl"
    write_jsonl(pairs, str(out))
    assert len(read_jsonl(str(out))) == len(pairs)
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_index.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.index'`.

- [ ] **Step 4: Implement index.py**

`spike/fingerprint/fp/index.py`:
```python
import json
import sys
from dataclasses import asdict, dataclass
from itertools import combinations
from pathlib import Path
from typing import Iterator

from datasketch import MinHash

from fp.extract import FunctionRecord, extract_functions
from fp.minhash import LshIndex, estimated_jaccard, minhash_of
from fp.normalize import MIN_TOKENS, structural_hash, tokens
from fp.signature import Signature, gate, signature_of, similarity

EXCLUDE_DIRS = {"node_modules", "android", "ios", ".expo", "dist", "build", "coverage",
                "__mocks__", ".git", ".venv"}
SOURCE_SUFFIXES = {".ts", ".tsx"}


def iter_source_files(roots: list[str]) -> Iterator[str]:
    for root in roots:
        for p in Path(root).rglob("*"):
            if not p.is_file() or p.suffix not in SOURCE_SUFFIXES or p.name.endswith(".d.ts"):
                continue
            if any(part in EXCLUDE_DIRS for part in p.parts):
                continue
            yield str(p.resolve()).replace("\\", "/")


@dataclass
class Indexed:
    record: FunctionRecord
    toks: list[str]
    shash: str
    mh: MinHash
    sig: Signature


def build(roots: list[str]) -> list[Indexed]:
    items: list[Indexed] = []
    for file in iter_source_files(roots):
        source = Path(file).read_text(encoding="utf8", errors="replace")
        for rec in extract_functions(file, source):
            toks = tokens(rec.source)
            if len(toks) < MIN_TOKENS:
                continue
            items.append(Indexed(rec, toks, structural_hash(toks), minhash_of(toks), signature_of(rec.source)))
    return items


@dataclass
class CandidatePair:
    a_id: str
    b_id: str
    a_file: str
    b_file: str
    a_name: str
    b_name: str
    structural_match: bool
    jaccard: float
    sig_sim: float
    sig_gate: bool
    a_source: str
    b_source: str


def _pair(a: Indexed, b: Indexed) -> CandidatePair:
    return CandidatePair(
        a_id=a.record.id, b_id=b.record.id,
        a_file=a.record.file, b_file=b.record.file,
        a_name=a.record.name, b_name=b.record.name,
        structural_match=a.shash == b.shash,
        jaccard=estimated_jaccard(a.mh, b.mh),
        sig_sim=similarity(a.sig, b.sig),
        sig_gate=gate(a.sig, b.sig),
        a_source=a.record.source, b_source=b.record.source,
    )


def _nested(a: Indexed, b: Indexed) -> bool:
    if a.record.file != b.record.file:
        return False
    return (a.record.start_line <= b.record.start_line <= a.record.end_line) or \
           (b.record.start_line <= a.record.start_line <= b.record.end_line)


def candidate_pairs(items: list[Indexed], floor: float = 0.3) -> list[CandidatePair]:
    by_id = {i.record.id: i for i in items}
    seen: set[frozenset[str]] = set()
    pairs: list[CandidatePair] = []

    by_hash: dict[str, list[Indexed]] = {}
    for i in items:
        by_hash.setdefault(i.shash, []).append(i)
    for group in by_hash.values():
        for a, b in combinations(group, 2):
            key = frozenset((a.record.id, b.record.id))
            if key in seen or _nested(a, b):
                continue
            seen.add(key)
            pairs.append(_pair(a, b))

    lsh = LshIndex(threshold=floor)
    for i in items:
        lsh.add(i.record.id, i.mh)
    for i in items:
        for other_id in lsh.query(i.mh):
            if other_id == i.record.id:
                continue
            key = frozenset((i.record.id, other_id))
            if key in seen:
                continue
            other = by_id[other_id]
            if _nested(i, other):
                continue
            seen.add(key)
            pairs.append(_pair(i, other))
    pairs.sort(key=lambda p: (-int(p.structural_match), -p.jaccard))
    return pairs


def write_jsonl(pairs: list[CandidatePair], path: str) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf8") as f:
        for p in pairs:
            f.write(json.dumps(asdict(p)) + "\n")


def read_jsonl(path: str) -> list[CandidatePair]:
    with open(path, encoding="utf8") as f:
        return [CandidatePair(**json.loads(line)) for line in f if line.strip()]


if __name__ == "__main__":
    *roots, out = sys.argv[1:]
    items = build(roots)
    pairs = candidate_pairs(items)
    write_jsonl(pairs, out)
    print(f"functions={len(items)} pairs={len(pairs)} -> {out}")
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_index.py -q
```
Expected: `3 passed`.

- [ ] **Step 6: Run on the real repos and record timing**

```bash
cd <repo>/spike/fingerprint && time .venv/Scripts/python -m fp.index <home>/fasting-app/src <home>/fasting-app/app <home>/fasting-app/components <home>/strongspan/src <home>/food-data-platform/apps <home>/food-data-platform/packages data/candidates.jsonl
```
Expected: a line like `functions=NNNN pairs=MMMM`. Write both numbers and the wall time into README under a heading `## Run log`. If it takes longer than ten minutes, stop it, add `--max-files` handling by slicing `iter_source_files` output in `__main__`, and run on `fasting-app/src` alone first.

- [ ] **Step 7: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/index.py spike/fingerprint/tests/fixtures/repo spike/fingerprint/tests/test_index.py spike/fingerprint/README.md && git commit -m "spike: repo indexer and candidate pair emission"
```

---

### Task 7: Planted near-duplicates for recall

**Files:**
- Create: `spike/fingerprint/fp/mutate.py`
- Create: `spike/fingerprint/tests/test_mutate.py`

**Interfaces:**
- Consumes: `index.Indexed`, `index.build`, `index.candidate_pairs`, `extract.FunctionRecord`, `normalize.tokens`, `normalize.structural_hash`, `minhash.minhash_of`, `signature.signature_of`.
- Produces: `mutate.MUTATIONS: dict[str, Callable[[str, random.Random], str]]` with keys `rename`, `insert`, `literals`, `combined`; `mutate.mutate(source: str, kind: str, seed: int) -> str` (wraps a mutation with a seeded `Random`); `mutate.Planted` dataclass (`original_id: str, planted_id: str, kind: str`); `mutate.plant(items: list[Indexed], n: int, seed: int) -> tuple[list[Indexed], list[Planted]]` returning the items list extended with synthetic entries; `mutate.write_planted(planted, path)`, `mutate.read_planted(path)`; a `__main__` that takes roots, n, and two output paths (pairs jsonl, planted jsonl).

- [ ] **Step 1: Write the failing tests**

`spike/fingerprint/tests/test_mutate.py`:
```python
from pathlib import Path

from fp.index import build, candidate_pairs
from fp.mutate import MUTATIONS, mutate, plant

SRC = """async function loadUser(id: string) {
  const res = await fetch(`/api/users/${id}`);
  if (!res.ok) {
    throw new Error("user load failed");
  }
  const data = await res.json();
  cache.set(id, data);
  return data;
}"""

ROOT = str(Path(__file__).parent / "fixtures" / "repo")


def test_rename_changes_identifiers_only():
    out = mutate(SRC, "rename", seed=1)
    assert out != SRC
    assert "fetch(" in out and "res.json()" in out
    assert "loadUser" not in out or "id" not in out.split("(")[1]


def test_insert_adds_a_statement():
    out = mutate(SRC, "insert", seed=1)
    assert out.count("\n") == SRC.count("\n") + 1


def test_literals_changes_strings():
    out = mutate(SRC, "literals", seed=1)
    assert '"user load failed"' not in out


def test_all_mutation_kinds_registered():
    assert set(MUTATIONS) == {"rename", "insert", "literals", "combined"}


def test_plant_extends_items_and_pairs_find_them():
    items = build([ROOT])
    extended, planted = plant(items, n=2, seed=3)
    assert len(extended) == len(items) + 2 and len(planted) == 2
    pairs = candidate_pairs(extended, floor=0.3)
    pair_keys = {frozenset((p.a_id, p.b_id)) for p in pairs}
    for pl in planted:
        assert frozenset((pl.original_id, pl.planted_id)) in pair_keys
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_mutate.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.mutate'`.

- [ ] **Step 3: Implement mutate.py**

`spike/fingerprint/fp/mutate.py`:
```python
import json
import random
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable

from fp.extract import FunctionRecord
from fp.index import Indexed, build, candidate_pairs, write_jsonl
from fp.minhash import minhash_of
from fp.normalize import structural_hash, tokens
from fp.signature import signature_of

RESERVED = {"function", "async", "await", "const", "let", "var", "return", "if", "else", "for",
            "of", "in", "while", "throw", "new", "true", "false", "null", "undefined", "string",
            "number", "boolean", "void", "export", "default", "class", "this", "import", "from",
            "try", "catch", "finally", "switch", "case", "break", "continue", "typeof", "instanceof"}

_IDENT = re.compile(r"\b([A-Za-z_$][A-Za-z0-9_$]*)\b")
_STRING = re.compile(r'"([^"\\]|\\.)*"|\'([^\'\\]|\\.)*\'')


def _rename(source: str, rng: random.Random) -> str:
    names = [n for n in dict.fromkeys(_IDENT.findall(source)) if n not in RESERVED and len(n) > 1]
    targets = rng.sample(names, k=min(3, len(names))) if names else []
    out = source
    for i, name in enumerate(targets):
        out = re.sub(rf"\b{re.escape(name)}\b", f"{name}Alt{i}", out)
    return out


def _insert(source: str, rng: random.Random) -> str:
    lines = source.split("\n")
    candidates = [i for i, l in enumerate(lines) if l.rstrip().endswith("{")]
    if not candidates:
        return source
    at = rng.choice(candidates)
    indent = re.match(r"\s*", lines[at]).group(0) + "  "
    lines.insert(at + 1, f'{indent}console.log("planted");')
    return "\n".join(lines)


def _literals(source: str, rng: random.Random) -> str:
    counter = [0]

    def repl(m: re.Match) -> str:
        counter[0] += 1
        return f'"planted-{rng.randint(0, 9999)}-{counter[0]}"'

    return _STRING.sub(repl, source)


def _combined(source: str, rng: random.Random) -> str:
    return _insert(_literals(_rename(source, rng), rng), rng)


MUTATIONS: dict[str, Callable[[str, random.Random], str]] = {
    "rename": _rename,
    "insert": _insert,
    "literals": _literals,
    "combined": _combined,
}


def mutate(source: str, kind: str, seed: int) -> str:
    return MUTATIONS[kind](source, random.Random(seed))


@dataclass
class Planted:
    original_id: str
    planted_id: str
    kind: str


def plant(items: list[Indexed], n: int, seed: int) -> tuple[list[Indexed], list[Planted]]:
    rng = random.Random(seed)
    chosen = rng.sample(items, k=min(n, len(items)))
    kinds = list(MUTATIONS)
    extended = list(items)
    planted: list[Planted] = []
    for k, item in enumerate(chosen):
        kind = kinds[k % len(kinds)]
        new_src = mutate(item.record.source, kind, seed=seed + k)
        rec = item.record
        planted_file = f"{rec.file}__planted_{k}.ts"
        new_rec = FunctionRecord(
            id=f"{planted_file}:{rec.start_line}:{rec.name}Planted",
            file=planted_file, name=f"{rec.name}Planted",
            start_line=rec.start_line, end_line=rec.end_line, source=new_src,
        )
        toks = tokens(new_src)
        extended.append(Indexed(new_rec, toks, structural_hash(toks), minhash_of(toks), signature_of(new_src)))
        planted.append(Planted(rec.id, new_rec.id, kind))
    return extended, planted


def write_planted(planted: list[Planted], path: str) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf8") as f:
        for p in planted:
            f.write(json.dumps(asdict(p)) + "\n")


def read_planted(path: str) -> list[Planted]:
    with open(path, encoding="utf8") as f:
        return [Planted(**json.loads(line)) for line in f if line.strip()]


if __name__ == "__main__":
    *roots, n, pairs_out, planted_out = sys.argv[1:]
    items = build(roots)
    extended, planted = plant(items, n=int(n), seed=42)
    write_jsonl(candidate_pairs(extended), pairs_out)
    write_planted(planted, planted_out)
    print(f"functions={len(items)} planted={len(planted)} -> {pairs_out}, {planted_out}")
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_mutate.py -q
```
Expected: `5 passed`. If `test_rename_changes_identifiers_only` is brittle on the last assertion, replace it with `assert out != SRC` only; the point is that renaming happened.

- [ ] **Step 5: Run the planted pipeline on the real repos**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m fp.mutate <home>/fasting-app/src <home>/fasting-app/app <home>/fasting-app/components <home>/strongspan/src <home>/food-data-platform/apps <home>/food-data-platform/packages 100 data/candidates_planted.jsonl data/planted.jsonl
```
Expected: `functions=NNNN planted=100`. Append the line to the README run log.

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/mutate.py spike/fingerprint/tests/test_mutate.py spike/fingerprint/README.md && git commit -m "spike: planted near-duplicates for recall measurement"
```

---

### Task 8: Labelling loop

**Files:**
- Create: `spike/fingerprint/fp/label.py`

**Interfaces:**
- Consumes: `index.read_jsonl`, `index.CandidatePair`.
- Produces: `label.Label` dataclass (`pair_key: str` as `a_id|b_id` sorted, `label: str` in `dup`, `not`, `unsure`, `reason: str`), `label.read_labels(path) -> dict[str, Label]`, `label.append_label(path, label)`, `label.pair_key(p: CandidatePair) -> str`, `label.sample_for_labelling(pairs, n, seed) -> list[CandidatePair]` (stratified: a third structural matches, a third jaccard 0.6 to 1.0, a third jaccard 0.3 to 0.6), and a `__main__` that runs the interactive loop.

No unit test for the interactive loop; `sample_for_labelling` and the readers are exercised by Task 9's tests.

- [ ] **Step 1: Implement label.py**

`spike/fingerprint/fp/label.py`:
```python
import json
import random
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

from fp.index import CandidatePair, read_jsonl


@dataclass
class Label:
    pair_key: str
    label: str
    reason: str


def pair_key(p: CandidatePair) -> str:
    return "|".join(sorted((p.a_id, p.b_id)))


def read_labels(path: str) -> dict[str, Label]:
    if not Path(path).exists():
        return {}
    out: dict[str, Label] = {}
    with open(path, encoding="utf8") as f:
        for line in f:
            if line.strip():
                l = Label(**json.loads(line))
                out[l.pair_key] = l
    return out


def append_label(path: str, label: Label) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a", encoding="utf8") as f:
        f.write(json.dumps(asdict(label)) + "\n")


def sample_for_labelling(pairs: list[CandidatePair], n: int, seed: int) -> list[CandidatePair]:
    rng = random.Random(seed)
    structural = [p for p in pairs if p.structural_match]
    high = [p for p in pairs if not p.structural_match and p.jaccard >= 0.6]
    mid = [p for p in pairs if not p.structural_match and 0.3 <= p.jaccard < 0.6]
    per = max(1, n // 3)
    picked: list[CandidatePair] = []
    for bucket in (structural, high, mid):
        picked.extend(rng.sample(bucket, k=min(per, len(bucket))))
    rng.shuffle(picked)
    return picked


def _show(p: CandidatePair) -> None:
    print("=" * 100)
    print(f"A {p.a_file}  {p.a_name}")
    print(f"B {p.b_file}  {p.b_name}")
    print(f"structural={p.structural_match} jaccard={p.jaccard:.2f} sig_sim={p.sig_sim:.2f} gate={p.sig_gate}")
    print("-" * 48 + " A " + "-" * 48)
    print(p.a_source[:2500])
    print("-" * 48 + " B " + "-" * 48)
    print(p.b_source[:2500])


if __name__ == "__main__":
    candidates_path, labels_path, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    pairs = read_jsonl(candidates_path)
    done = read_labels(labels_path)
    todo = [p for p in sample_for_labelling(pairs, n, seed=7) if pair_key(p) not in done]
    print(f"{len(todo)} pairs to label. d=dup n=not u=unsure q=quit. Add a reason after a space.")
    for p in todo:
        _show(p)
        raw = input("> ").strip()
        if raw.startswith("q"):
            break
        code, _, reason = raw.partition(" ")
        label = {"d": "dup", "n": "not", "u": "unsure"}.get(code)
        if label is None:
            print("skipped")
            continue
        append_label(labels_path, Label(pair_key(p), label, reason))
    print(f"labels now: {len(read_labels(labels_path))}")
```

- [ ] **Step 2: Run a five-pair dry run to prove the loop works**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m fp.label data/candidates.jsonl data/labels_dryrun.jsonl 5
```
Answer `u` to each. Expected: `labels now: 5`. Then delete `data/labels_dryrun.jsonl`.

- [ ] **Step 3: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/label.py && git commit -m "spike: labelling loop"
```

---

### Task 9: Evaluator and report

**Files:**
- Create: `spike/fingerprint/fp/evaluate.py`
- Create: `spike/fingerprint/tests/test_evaluate.py`

**Interfaces:**
- Consumes: `index.CandidatePair`, `index.read_jsonl`, `mutate.Planted`, `mutate.read_planted`, `label.Label`, `label.read_labels`, `label.pair_key`.
- Produces: `evaluate.predicted(p: CandidatePair, t: float, require_gate: bool) -> bool` (True when `structural_match`, or `jaccard >= t` and (`sig_gate` or not `require_gate`)); `evaluate.recall_at(pairs, planted, t, require_gate) -> float`; `evaluate.precision_at(pairs, labels, t, require_gate) -> tuple[float, int]` (precision and number of labelled predicted pairs, `unsure` excluded); `evaluate.sweep(pairs_planted, planted, pairs_real, labels, require_gate) -> list[dict]` over `t` in 0.30 to 0.95 step 0.05; `evaluate.choose(rows, target_recall=0.9) -> dict | None` (highest `t` whose recall is at least the target); `evaluate.write_report(path, rows_gate, rows_nogate, chosen_gate, chosen_nogate, counts)`; `__main__`.

Definitions written into the report:
- Recall at t: fraction of planted pairs that are predicted at t.
- Precision at t: among labelled pairs that are predicted at t, the fraction labelled `dup`. `unsure` labels are dropped from both numerator and denominator.
- The headline number: precision at the highest t where recall is at least 0.90, with the signature gate on. The no-gate row is reported for comparison.

- [ ] **Step 1: Write the failing tests**

`spike/fingerprint/tests/test_evaluate.py`:
```python
from fp.evaluate import choose, precision_at, predicted, recall_at, sweep
from fp.index import CandidatePair
from fp.label import Label, pair_key
from fp.mutate import Planted


def cp(a, b, structural=False, jaccard=0.0, gate=True):
    return CandidatePair(a_id=a, b_id=b, a_file="fa", b_file="fb", a_name=a, b_name=b,
                         structural_match=structural, jaccard=jaccard, sig_sim=0.5, sig_gate=gate,
                         a_source="", b_source="")


def test_predicted_rules():
    assert predicted(cp("a", "b", structural=True), t=0.9, require_gate=True)
    assert predicted(cp("a", "b", jaccard=0.7), t=0.6, require_gate=True)
    assert not predicted(cp("a", "b", jaccard=0.5), t=0.6, require_gate=True)
    assert not predicted(cp("a", "b", jaccard=0.7, gate=False), t=0.6, require_gate=True)
    assert predicted(cp("a", "b", jaccard=0.7, gate=False), t=0.6, require_gate=False)


def test_recall_and_precision():
    pairs = [cp("o1", "p1", jaccard=0.8), cp("o2", "p2", jaccard=0.4), cp("x", "y", jaccard=0.9), cp("q", "r", jaccard=0.7)]
    planted = [Planted("o1", "p1", "rename"), Planted("o2", "p2", "combined")]
    assert recall_at(pairs, planted, t=0.6, require_gate=True) == 0.5
    assert recall_at(pairs, planted, t=0.3, require_gate=True) == 1.0
    labels = {
        pair_key(pairs[2]): Label(pair_key(pairs[2]), "dup", ""),
        pair_key(pairs[3]): Label(pair_key(pairs[3]), "not", ""),
        pair_key(pairs[1]): Label(pair_key(pairs[1]), "unsure", ""),
    }
    prec, n = precision_at(pairs, labels, t=0.6, require_gate=True)
    assert n == 2 and prec == 0.5


def test_sweep_and_choose():
    pairs = [cp("o1", "p1", jaccard=0.8), cp("o2", "p2", jaccard=0.5)]
    planted = [Planted("o1", "p1", "rename"), Planted("o2", "p2", "insert")]
    labels = {pair_key(pairs[0]): Label(pair_key(pairs[0]), "dup", "")}
    rows = sweep(pairs, planted, pairs, labels, require_gate=True)
    assert rows[0]["t"] == 0.3 and rows[-1]["t"] == 0.95
    chosen = choose(rows, target_recall=0.9)
    assert chosen is not None and chosen["t"] == 0.5 and chosen["recall"] == 1.0
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest tests/test_evaluate.py -q
```
Expected: FAIL with `ModuleNotFoundError: No module named 'fp.evaluate'`.

- [ ] **Step 3: Implement evaluate.py**

`spike/fingerprint/fp/evaluate.py`:
```python
import sys
from datetime import date

from fp.index import CandidatePair, read_jsonl
from fp.label import Label, pair_key, read_labels
from fp.mutate import Planted, read_planted

THRESHOLDS = [round(0.30 + 0.05 * i, 2) for i in range(14)]  # 0.30 .. 0.95


def predicted(p: CandidatePair, t: float, require_gate: bool) -> bool:
    if p.structural_match:
        return True
    if p.jaccard < t:
        return False
    return p.sig_gate or not require_gate


def recall_at(pairs: list[CandidatePair], planted: list[Planted], t: float, require_gate: bool) -> float:
    if not planted:
        return 0.0
    hit_keys = {pair_key(p) for p in pairs if predicted(p, t, require_gate)}
    found = sum(1 for pl in planted if "|".join(sorted((pl.original_id, pl.planted_id))) in hit_keys)
    return found / len(planted)


def precision_at(pairs: list[CandidatePair], labels: dict[str, Label], t: float, require_gate: bool) -> tuple[float, int]:
    tp = fp = 0
    for p in pairs:
        if not predicted(p, t, require_gate):
            continue
        l = labels.get(pair_key(p))
        if l is None or l.label == "unsure":
            continue
        if l.label == "dup":
            tp += 1
        else:
            fp += 1
    n = tp + fp
    return (tp / n if n else 0.0), n


def sweep(pairs_planted: list[CandidatePair], planted: list[Planted], pairs_real: list[CandidatePair],
          labels: dict[str, Label], require_gate: bool) -> list[dict]:
    rows = []
    for t in THRESHOLDS:
        prec, n = precision_at(pairs_real, labels, t, require_gate)
        rows.append({"t": t, "recall": recall_at(pairs_planted, planted, t, require_gate),
                     "precision": prec, "labelled_predicted": n})
    return rows


def choose(rows: list[dict], target_recall: float = 0.9) -> dict | None:
    ok = [r for r in rows if r["recall"] >= target_recall]
    return max(ok, key=lambda r: r["t"]) if ok else None


def _table(rows: list[dict]) -> str:
    lines = ["| t | recall | precision | labelled predicted |", "|---|---|---|---|"]
    for r in rows:
        lines.append(f"| {r['t']:.2f} | {r['recall']:.2f} | {r['precision']:.2f} | {r['labelled_predicted']} |")
    return "\n".join(lines)


def write_report(path: str, rows_gate: list[dict], rows_nogate: list[dict], chosen_gate: dict | None,
                 chosen_nogate: dict | None, counts: dict) -> None:
    def headline(c: dict | None) -> str:
        if c is None:
            return "recall target not reached at any threshold"
        return f"precision {c['precision']:.2f} at t={c['t']:.2f} (recall {c['recall']:.2f}, {c['labelled_predicted']} labelled pairs)"

    verdict = "UNKNOWN"
    if chosen_gate is not None:
        verdict = "PASS: ship already-exists in version one" if chosen_gate["precision"] >= 0.85 \
            else "FAIL: move already-exists to release two"
    body = f"""# Fingerprinting spike report ({date.today().isoformat()})

## Headline
With signature gate: {headline(chosen_gate)}
Without signature gate: {headline(chosen_nogate)}

**Verdict: {verdict}**

## Definitions
- Recall at t: fraction of {counts['planted']} planted near-duplicates retrieved at threshold t.
- Precision at t: among labelled candidate pairs predicted at t, fraction labelled dup. Unsure labels excluded.
- Headline: precision at the highest t with recall >= 0.90, gate on.
- Signals: structural hash (always predicts), MinHash Jaccard over 5-token shingles with 128 permutations, signature gate (param count equal or callee Jaccard >= 0.5).

## Counts
functions={counts['functions']} candidate_pairs={counts['pairs']} labelled={counts['labelled']} planted={counts['planted']}

## Sweep, gate on
{_table(rows_gate)}

## Sweep, gate off
{_table(rows_nogate)}

## Thresholds to carry into the engine (spec section 3.3)
- SHINGLE_K = 5, NUM_PERM = 128
- jaccard_threshold = {chosen_gate['t'] if chosen_gate else 'n/a'}
- signature_gate = {'on' if chosen_gate and chosen_gate['precision'] >= (chosen_nogate['precision'] if chosen_nogate else 0) else 'off'}
- MIN_TOKENS = 40
"""
    with open(path, "w", encoding="utf8") as f:
        f.write(body)


if __name__ == "__main__":
    candidates_real, candidates_planted, planted_path, labels_path, out = sys.argv[1:6]
    pairs_real = read_jsonl(candidates_real)
    pairs_planted = read_jsonl(candidates_planted)
    planted = read_planted(planted_path)
    labels = read_labels(labels_path)
    rows_gate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=True)
    rows_nogate = sweep(pairs_planted, planted, pairs_real, labels, require_gate=False)
    cg, cn = choose(rows_gate), choose(rows_nogate)
    functions = len({i for p in pairs_real for i in (p.a_id, p.b_id)})
    write_report(out, rows_gate, rows_nogate, cg, cn,
                 {"functions": functions, "pairs": len(pairs_real), "labelled": len(labels), "planted": len(planted)})
    print(open(out, encoding="utf8").read().split("## Definitions")[0])
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m pytest -q
```
Expected: all tests pass (smoke 1, extract 3, normalize 4, minhash 3, signature 3, index 3, mutate 5, evaluate 3 = `25 passed`).

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add spike/fingerprint/fp/evaluate.py spike/fingerprint/tests/test_evaluate.py && git commit -m "spike: evaluator, threshold sweep and report"
```

---

### Task 10: Label, evaluate, decide

**Files:**
- Create: `spike/fingerprint/REPORT.md` (generated)
- Modify: `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md` section 3.3 and section 4.1 (`already-exists` line)
- Modify: `spike/fingerprint/README.md` run log

**Interfaces:**
- Consumes: everything above.
- Produces: the one number and the spec update.

- [ ] **Step 1: Label at least 200 real candidate pairs**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m fp.label data/candidates.jsonl data/labels.jsonl 240
```
Labelling rule, applied the same way to every pair: `dup` means a maintainer would reasonably replace one with a call to the other or a shared helper; `not` means they do different jobs even if they look alike (for example two React components with the same shape but different content); `unsure` when the labeller cannot decide in thirty seconds. Give a reason of at least three words for every `dup` and `not`. If Claude labels, it writes the reason; the founder then re-labels a random 50 with the labels hidden, and if agreement is below 90 percent, all 240 are relabelled by the founder before the number is reported.

- [ ] **Step 2: Generate the report**

```bash
cd <repo>/spike/fingerprint && .venv/Scripts/python -m fp.evaluate data/candidates.jsonl data/candidates_planted.jsonl data/planted.jsonl data/labels.jsonl REPORT.md
```
Expected: the headline block printed, `REPORT.md` written with a PASS or FAIL verdict.

- [ ] **Step 3: One tuning pass, at most**

If the verdict is FAIL and the sweep shows precision climbs sharply just above the chosen t, the single permitted change is `MIN_TOKENS` from 40 to 60 in `fp/normalize.py` (plus the constant test in `tests/test_normalize.py`). Re-run Task 6 step 6, Task 7 step 5, and step 2 above. Record both results in `REPORT.md` under a heading `## Tuning pass`. No second pass, whatever the outcome.

- [ ] **Step 4: Copy the result into the spec**

In `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, section 3.3, replace the sentence "Thresholds live in one config struct and are set by the spike, not by hand." with the exact values from `REPORT.md` ("Thresholds to carry into the engine"), and add one line: "Spike result (date): precision X at recall Y on N labelled pairs; see spike/fingerprint/REPORT.md." In section 4.1, on the `already-exists` line, replace "Gated on the spike (section 10)" with "Ships in version one" on PASS or "Moved to release two, see 4.2" on FAIL, and on FAIL add the rule to section 4.2.

- [ ] **Step 5: Commit and open the pull request**

```bash
cd <repo> && git add spike/fingerprint/REPORT.md spike/fingerprint/README.md docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md && git commit -m "spike: fingerprinting result and spec thresholds"
```
The repo has no remote yet. Creating one on GitHub is a founder action (private repo, same account as kcalbase). Once the remote exists: `git push -u origin spike/fingerprint` and open a pull request titled "Fingerprinting spike: result and thresholds" whose body is the headline block from `REPORT.md`. Until then the branch stays local and the founder reads `REPORT.md` directly.

---

## Self-review

**Spec coverage.** Section 3.3 signals: structural hash (Task 3), MinHash with LSH (Task 4), signature vector (Task 5), candidate rule combining them (Task 6). Section 10.4: throwaway language (Python), founder's repos (Task 6 step 6), hand-labelled sample (Task 10 step 1), one number precision at fixed recall (Task 9 definitions), 85 percent decision with the fallback to release two (Task 10 step 4), two-week box (Global Constraints). Section 3.4 performance targets are not part of the spike and are not claimed here; the README run log records Python timing for curiosity only.

**Placeholder scan.** No TBD, TODO, or "handle edge cases" phrasing. Every code step has the code. The only deliberately open value is the wall-time number the executor records after running.

**Type consistency.** `FunctionRecord` fields used in Tasks 6 and 7 match Task 2. `Indexed` fields used in Task 7 match Task 6. `CandidatePair` fields used in Tasks 8 and 9 match Task 6. `Planted` fields used in Task 9 match Task 7. `Label` and `pair_key` used in Task 9 match Task 8. `MIN_TOKENS` is defined once in `normalize.py` and imported by `index.py`. `LshIndex(threshold=...)` constructor matches between Tasks 4 and 6. `mutate.MUTATIONS` values take `(source, rng)` and `mutate.mutate` wraps them with a seeded `Random`, matching the test which calls `mutate(SRC, kind, seed=1)`.
