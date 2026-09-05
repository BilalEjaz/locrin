# Fingerprinting spike (throwaway)

Measures whether structural hash + MinHash + signature vector finds near-duplicate
TypeScript functions at >= 85% precision at 90% recall on the founder's repos.

Run: `.venv/Scripts/python -m pytest` for tests. Pipeline commands are in
`docs/superpowers/plans/2026-09-05-fingerprint-spike.md`. The result lives in `REPORT.md`.

Nothing in here ships. The production engine is Rust.
