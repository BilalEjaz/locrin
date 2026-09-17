# Locrin GitHub Action

Runs `locrin check` on a pull request or a deployment, posts one verdict comment,
uploads SARIF to code scanning, and fails the job on a BLOCK verdict.

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: BilalEjaz/locrin/action@v0.6.1
  with:
    version: v0.6.1
```

The default pull-request path (`--base`) diffs against the merge base, so the
checkout needs `fetch-depth: 0`, or enough history to reach that merge base; a
shallow checkout has none and the check exits 2.

## Inputs

| Input | Default | Description |
|---|---|---|
| `version` | `latest` | Release tag (`v0.6.1`), `latest`, or `local` for a `locrin` already on PATH. `latest` follows the newest release, so pin it to the action ref as the examples do |
| `path` | `.` | Repository-relative directory to check |
| `base` | `${{ github.event.pull_request.base.sha }}` | Base ref for the pull-request view (files that differ from the merge base). Set it to `""` to check every file under `path` |
| `since` | `""` | Ref for the deployment gate (files changed by `REF..HEAD`); overrides `base` |
| `comment` | `true` | Post or update one summary comment on the pull request |
| `sarif` | `true` | Upload SARIF to code scanning |
| `offline` | `false` | Never touch the network (vulnerable-dependency uses its snapshot or skips) |
| `fail-on-block` | `true` | Fail the step when the verdict is BLOCK |
| `token` | `${{ github.token }}` | Token for the comment and the release download |
| `download-token` | `""` | Optional token for the release download; only needed if the repository ever needs one. Defaults to `token` |

## Outputs

| Output | Description |
|---|---|
| `exit-code` | The exit code of the `locrin check` run (`0` clean, `1` block, `2+` error) |
| `status` | `pass`, `advisory`, `block` or `error` |
| `sarif-file` | Absolute path to the SARIF file the run wrote |
| `comment-file` | Absolute path to the Markdown summary the run wrote |

## Permissions

```yaml
permissions:
  contents: read          # always: the checkout of your own repository
  pull-requests: write    # only when comment is true
  security-events: write  # only when sarif is true
```

The workflow's own `token` downloads the release and posts the comment; the
optional `download-token` overrides it for the download alone.

The comment and the SARIF steps are both `continue-on-error`: a fork pull
request, whose token is read-only, logs a `::warning::` instead of turning the
check red. Code scanning needs GitHub Advanced Security on a repository that
is not public.

## One comment, edited in place

The Markdown summary starts with the marker `<!-- locrin-report -->` on its own
first line. The action edits the first pull-request comment whose body starts
with it, and creates one only when no marked comment exists. When the lookup
itself fails the step warns and posts nothing, rather than risk a duplicate.

## Examples

- `examples/pull-request.yml`: the gate on every pull request, comment and SARIF.
- `examples/deploy-gate.yml`: the deployment gate, `since` against the last tag.
- `examples/fastlift.yml`: the FastLift workflow, ready to copy into that repo.

Supported runners: Linux x64, macOS arm64, macOS x64, Windows x64. Checksums are
verified against the release `SHA256SUMS` before the archive is unpacked.
