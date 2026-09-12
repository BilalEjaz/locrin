# Contributing

## Build and test

    cargo build
    cargo test --workspace
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    bash scripts/tests/run.sh

All five must pass before a pull request is reviewed. CI runs them on Linux and
Windows.

## Rules and the precision gate

A rule ships only when it reaches 85 percent precision on the labelled corpus,
per rule and per rule-language pair. The corpus is private (it is mined from
private repositories), so a pull request that adds or changes a rule must
include, in its description, at least ten labelled snippets (five that should
fire, five that should not) so the maintainer can extend the corpus and run the
gate. A rule under the line ships off by default or does not ship.

Rule ids, finding ids and exit codes are contracts. Changing how an id is
computed needs a release note and a bump of `RULES_REVISION`.

## Style

Rust is formatted by rustfmt (rustfmt.toml sets the width). Prose in comments,
docs and release notes uses plain sentences and no em dashes.

## Commits

Write the commit message in your own words. Do not add attribution trailers for
tools or assistants; the author line is the author.

## Reporting false positives

The most useful report is a false positive: use the issue template, include the
rule id, the finding id from `locrin check --json`, the snippet, and why it is
wrong.
