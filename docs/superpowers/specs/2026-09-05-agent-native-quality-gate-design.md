# Agent-native quality gate: design spec

Date: 2026-09-05
Status: APPROVED IN CONVERSATION, awaiting founder read of this file
Research basis: docs/research/2026-09-05-SONARQUBE-MARKET-RESEARCH.md
Product name: not chosen. "the engine" and "the gate" are placeholders throughout.

## 1. Decisions locked

| Decision | Choice | Why |
|---|---|---|
| Wedge | Agent-native quality gate (research wedge A) | Sonar cannot drop LOC pricing or become a local binary; erosion niche is held only by tiny OSS tools |
| First customer | Small companies with heavy agent use, bottom-up; founder's own company dogfoods on daily deploys | Founder decision |
| Order of surfaces | Agent-facing (Claude Code hooks + MCP) first, CI reporter second, editor LSP third | Founder decision |
| Business model | Open core: free engine, CLI, core rules, hooks, MCP, basic GitHub Action; paid hosted layer and paid rule packs by licence key | Distribution first; Sonar's crippled free tier is its most-cited grievance |
| Engine stack | Rust single binary, tree-sitter parsers, embedded SQLite index (approach A) | Only option that delivers sub-second, zero-runtime, air-gapped honestly |
| AI at runtime | None. LLM is optional, off by default, explain-and-fix only, never the source of a finding | Cost predictability; adoptable by teams that ban AI tools |
| Pricing unit | Flat per repo (hosted), per org bundle, licence key for packs. Never per line, never per seat | Structural moat against Sonar; per-seat is collapsing market-wide |
| Languages v1 | TypeScript, TSX, JavaScript | Founder's repos; where agents dominate |
| Languages v2 | PHP, Python behind a config flag, same pipeline | ServeNest, wider market |

## 2. Non-goals for version one
- Semantic (Type-4) clone detection: different algorithms, same behaviour. Out of scope and stated publicly.
- Whole-program type checking. We do not run the TypeScript compiler. Resolution is heuristic via tsconfig paths and package exports.
- Business-logic security (broken access control, insecure design) beyond framework-specific rules. Stated as a limit, not hidden.
- GitLab and Bitbucket reporters. Added on request.
- Editor extensions and LSP. Third phase.
- Any dashboard. Hosted layer is a later phase; the boundary is designed now (section 8).

## 3. Architecture

One Rust workspace, one shipped binary. Crates:

- `core`: parsing, index, incremental graph, fingerprinting. No rule logic.
- `rules`: analyzers as pure functions over the index. Each rule is its own module with fixtures.
- `reporters`: agent JSON, SARIF 2.1.0, terminal.
- `cli`: commands, hook entry points, `init`.
- `mcp`: MCP server exposing five tools (section 6).

Distribution: npm package that downloads the platform binary (the oxlint and Biome pattern), Homebrew tap, cargo install, direct download. One-line install on every path.

### 3.1 Parsing
Tree-sitter grammars for TypeScript, TSX, JavaScript. Grammar versions pinned. Parse errors in a file degrade that file to "unparsed" and are reported once, never as findings.

### 3.2 Index
SQLite database per repository in the user cache directory, keyed by canonical repo path. Never inside the repo, never committed. Schema:

- `files`: path, content hash, language, parse status.
- `symbols`: id, file, kind (function, method, class, const, type), name, span, exported flag, signature hash.
- `edges`: from symbol or file, to symbol or file, kind (import, re-export, call), resolution status (resolved, external, unresolved).
- `fingerprints`: symbol id, structural hash, MinHash signature, LSH band keys, signature vector.
- `findings_cache`: file hash, rule id, serialized findings.

Incremental rule: a changed file is re-parsed and re-fingerprinted; its reverse-import dependents are re-evaluated for cross-file rules only. Everything else is served from cache.

Resolution: tsconfig `paths` and `baseUrl`, package.json `exports` and workspaces, barrel files followed one level. Known blind spots: deep re-export chains, dynamic imports with computed paths. Recorded as `unresolved`, never guessed.

### 3.3 Fingerprinting (the "already exists" signal)
Per function or method, three signals:
1. Structural hash of the normalised syntax tree: identifiers and literals replaced by kind placeholders. Catches renamed copies (Type-2).
2. Token-shingle MinHash (shingle size and band count set by the spike) with locality-sensitive hashing for candidate retrieval. Catches near-copies with edits (Type-3), which is what agents produce.
3. Signature vector: parameter count, parameter type tokens, return shape, set of external callees. Used to rank and to break ties, never alone.

A candidate is reported when the structural hashes match, or the MinHash estimated similarity exceeds the spike-set threshold and the signature vector agrees. Thresholds carried from the spike (spike/fingerprint/REPORT.md, "Thresholds to carry into the engine"): SHINGLE_K = 5, NUM_PERM = 128, jaccard_threshold = 0.70, signature_gate = on, MIN_TOKENS = 40; they live in one config struct.
Spike result (2026-09-05): precision 0.69 at recall 0.91 on 240 labelled pairs (116 predicted at t=0.70); see spike/fingerprint/REPORT.md (provisional pending founder spot-check).

### 3.4 Performance targets (benchmark tests, build fails on miss)
| Measure | Target |
|---|---|
| Cold index, 3,000-file TypeScript repo | under 5 s |
| Warm single-file check (hook path) | under 300 ms |
| Warm diff check, typical PR (30 files) | under 1 s |
| Peak memory | under 300 MB |
| Binary start to first output | under 50 ms |

### 3.5 Baseline
On first run the engine writes a baseline file at the repo root containing every existing finding id. The baseline is committed. Default behaviour reports only findings not in the baseline, plus baselined findings whose severity increased. See section 7 for the file format.

## 4. Rule set

Every finding carries: rule id, category (erosion or security), OWASP 2021 category and CWE id where applicable, severity (high, medium, low), confidence (high or medium), file and span, one-line evidence, one-line fix instruction, related symbols.

### 4.1 Version one (ship only if 85 percent precision on the corpus)

Erosion pack, free:
- `already-exists`: new or changed function near-duplicates an existing one. Moved to release two, see 4.2. Reports the existing symbol and suggests reuse.
- `dead-export`: exported symbol imported nowhere. Entry points from package.json (`main`, `exports`, `bin`), framework conventions (Next.js `app/` and `pages/`, Expo Router `app/`), and config.
- `dead-file`: file imported nowhere and not an entry point.
- `unreachable`: code after unconditional return, throw, break, continue.
- `unused-import`.
- `swallowed-error`: empty catch, catch that only logs and returns undefined where the caller uses the result, promise created without await, then, or catch.
- `boundary-violation` (config-driven half only): import crosses a forbidden direction declared in config.
- `test-no-assert`: test file or test case with zero assertions.
- `test-newly-skipped`: a test changed from active to skipped in this diff.
- `leftover-commented-code`: block of three or more commented-out statements.
- `leftover-debug`: console.log, debugger, and equivalents outside allowed files.
- `leftover-agent-marker`: TODO or FIXME introduced in this diff without an issue reference.

Security pack, core subset free:
- `secret-exposed`: pattern set (100 providers at launch) plus entropy check. Always blocks. Cannot be downgraded by config. OWASP A02, CWE-798.
- `weak-crypto`: MD5 or SHA-1 for passwords, Math.random for tokens or ids, hardcoded IVs. A02, CWE-327 and CWE-338.
- `injection-sink`: eval and new Function with non-literal input; child_process exec with template or concatenated strings; raw SQL built by concatenation or template with an identifier inside. Intra-file source tracking only. A03, CWE-78, CWE-89, CWE-95.
- `html-injection`: dangerouslySetInnerHTML or innerHTML with a non-literal. A03, CWE-79.
- `vulnerable-dependency`: lockfile packages checked against OSV. Online batch query by default; offline mode uses a cached OSV snapshot with an age warning. A06, CWE-1395.
- Framework rules, Supabase: service-role key referenced in client-side code; table created or altered without row-level security in migration files. A01, CWE-284.
- Framework rules, Express: route handler registered without any middleware when config declares an auth middleware name; `cors()` with wildcard origin on a route marked authenticated; cookies set without httpOnly or secure. A01, A05, CWE-306, CWE-614.

### 4.2 Held for release two
- `already-exists`: new or changed function near-duplicates an existing one. The spike measured precision 0.69 at recall 0.91 against the 0.85 bar (spike/fingerprint/REPORT.md, provisional pending founder spot-check), so it waits here until the fingerprint clears the bar. Reports the existing symbol and suggests reuse.
- `pattern-fragmentation`: multiple wrappers around the same external. Needs clustering that the corpus proves.
- `boundary-inferred`: conventions inferred from the import graph.
- `test-mock-only`: tests that assert only on mock calls.
- Wider taint: cross-function and cross-file sources for injection and SSRF.
- Framework packs: Next.js, Expo, Laravel, with their languages.

### 4.3 Blocking policy
- High-confidence findings block by default. Medium-confidence findings are advisory and never block.
- Per-rule severity can be overridden in config, except `secret-exposed`, which always blocks.
- A rule under 85 percent precision on the corpus cannot merge into the engine (section 10).

## 5. Agent integration

### 5.1 `init`
Detects the repo and package manager, writes the Claude Code hook entries into the project settings file and the MCP entry into the project MCP config, creates the config file with defaults, runs the first index, writes the baseline, and prints every file it touched. No global changes.

### 5.2 Three moments
1. Before writing: MCP tool `find_existing` (section 6) returns existing symbols matching an intent or signature.
2. After each edit: Claude Code PostToolUse hook on Edit, Write, and MultiEdit runs a single-file check. Budget 300 ms. High-confidence blocking findings are returned as hook feedback the agent must address. Advisory findings are attached as context only.
3. Before stopping: Claude Code Stop hook runs a working-tree diff check. If blocking findings remain, the agent is instructed to continue. Hard cap of three rounds per session, tracked in a session file; after three, the hook stops intervening and prints a summary for the human.

### 5.3 Token thrift
- Payloads contain rule, location, one line of evidence, one line of fix instruction, related symbols. Never file contents.
- Capped at the ten highest-severity findings per response, with a count of the rest.
- Verdict first, list second.

### 5.4 Other agents
Cursor and Codex: MCP server plus a pre-commit hook from day one. Native hooks added after the Claude Code path is proven.

## 6. MCP server: five tools
| Tool | Input | Output |
|---|---|---|
| `check_changes` | optional list of paths, or diff base ref | verdict plus capped findings |
| `find_existing` | intent text and/or a signature sketch (name, params, return) | ranked existing symbols with file, line, one-line summary |
| `explain_finding` | finding id | rule rationale, evidence, fix instruction, related symbols |
| `accept_finding` | finding id, reason | adds to baseline with author and timestamp; returns confirmation |
| `status` | none | index freshness, config summary, baseline size, last verdict |

`find_existing` ranks by signature vector similarity first and name token overlap second. It does not use an LLM.

## 7. Output contract, config, baseline

### 7.1 Finding record (all reporters)
`id` (hash of rule id, symbol id or normalised span, and file path; stable across line shifts), `rule`, `category`, `owasp` (optional), `cwe` (optional), `severity`, `confidence`, `file`, `span`, `evidence`, `fix`, `related` (list of symbol references).

### 7.2 Verdict
`status` (pass, advisory, block), counts by severity and confidence, `duration_ms`, `findings` (capped for agents, full for SARIF and CI), `truncated` count.

### 7.3 Reporters
- Agent JSON: compact, capped.
- SARIF 2.1.0: full, for GitHub code scanning and third-party tools.
- Terminal: grouped by file, colour, fix instruction inline.

### 7.4 Exit codes
0 pass or advisory, 1 block, 2 engine error.

### 7.5 Config file
One file at the repo root, committed. Sections: languages, entry points, boundaries (allowed and forbidden import directions between path globs), rule overrides (severity, disable), exclusions (globs for generated code), blocking policy, framework hints (auth middleware names). Zero-config defaults must work.

### 7.6 Baseline file
Committed at the repo root. One entry per accepted finding: id, rule, file, reason, author, date. Reviewable in pull requests. `accept_finding` and `baseline accept` on the CLI append to it.

## 8. CI reporter and hosted boundary (phase two, boundary fixed now)

### 8.1 GitHub Action
Same binary, `check --base <ref>`. Posts one summary comment (verdict, counts, top findings), edits it on subsequent pushes, uploads SARIF, sets a check status usable by branch protection.

### 8.2 Deployment gate
`check --since <ref>` runs on everything merged since the last deploy tag and fails the pipeline on block. This is the founder's daily dogfood path from week one.

### 8.3 Free versus hosted
Everything above is free, no account. Hosted adds only what needs memory across runs and people: trend history, erosion score over time, team rule overrides, PR comments with history, org dashboard, SSO, audit log. The engine sends finding records only, never source, and only when a token is configured. No token, no network, ever.

### 8.4 Paid packs
Full security pack and compliance PDF reports unlock offline by licence key. Packs and hosted are separate purchases.

## 9. Error handling
- Parse failure: file marked unparsed, one warning, no findings from that file, exit code unaffected.
- Index corruption or schema mismatch: rebuild from scratch automatically, log once.
- OSV unreachable: fall back to cached snapshot with age warning; if no snapshot, skip the rule with a warning, never block on network.
- Hook timeout (over 2 s): return pass with a warning so the agent is never stalled by the gate.
- Any engine panic: exit 2 with a one-line message and a path to a debug log. Never exit 1 on an engine bug.

## 10. Testing and the corpus

### 10.1 Per rule, test first
Each rule ships with fixture directories: must-flag, must-not-flag, edge. The rule is complete when all pass and no other rule regresses. Rules are pure functions over the index; these tests run in milliseconds.

### 10.2 Labelled corpus
Mined from agent-written commits in FastLift, StrongSpan, Kcalbase, plus selected public repos with heavy agent activity. Hand-labelled. Target for version one: a few hundred diffs. Every engine pull request runs the corpus and reports precision and recall per rule. Under 85 percent precision, the rule cannot merge.

### 10.3 Performance and integration
Section 3.4 targets are benchmark tests that fail the build. Hooks are tested by replaying recorded Claude Code hook payloads and asserting exact feedback. The MCP server has protocol conformance tests plus a scripted agent session that must reach a clean verdict in under three rounds.

### 10.4 The spike (before the engine)
Two weeks maximum, throwaway code in whatever language is fastest, on the section 3.3 fingerprinting approach, against a hand-labelled duplicate sample from the founder's repos. Output is one number: precision at a fixed recall. At or above 85 percent, `already-exists` ships in version one and the thresholds are copied into the engine. Below, `already-exists` moves to release two and version one launches on the rest of section 4.1.

### 10.5 Public benchmark
Once stable, open-source the labelled dataset and scoring harness and publish results for the engine, aislop, Drift, and Sonar's rule set on the same data.

## 11. Phasing
1. Spike: fingerprinting precision (section 10.4).
2. Version one: core, index, section 4.1 rules, agent JSON and terminal reporters, CLI, `init`, Claude Code hooks, MCP server, pre-commit hook, baseline. Dogfood on the founder's repos.
3. Phase two: GitHub Action, SARIF upload, deployment gate, PHP and Python behind the flag.
4. Phase three: hosted layer, licence-key packs, LSP and editor extensions, public benchmark.

Naming, domain, and pricing numbers are separate decisions and are not part of this spec.
