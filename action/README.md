# Locrin GitHub Action

Runs `locrin check` on a pull request or a deployment, posts one verdict comment,
uploads SARIF to code scanning, and fails the job on a BLOCK verdict.

```yaml
- uses: BilalEjaz/locrin/action@v0.3.0
  with:
    version: v0.3.0
```

## Inputs

| Input | Default | Description |
|---|---|---|
| `version` | `latest` | Release tag (`v0.3.0`), `latest`, or `local` for a `locrin` already on PATH |
| `path` | `.` | Repository-relative directory to check |
| `base` | `${{ github.event.pull_request.base.sha }}` | Base ref for the pull-request view (files that differ from the merge base). Set it to `""` to check every file under `path` |
| `since` | `""` | Ref for the deployment gate (files changed by `REF..HEAD`); overrides `base` |
| `comment` | `true` | Post or update one summary comment on the pull request |
| `sarif` | `true` | Upload SARIF to code scanning |
| `offline` | `false` | Never touch the network (vulnerable-dependency uses its snapshot or skips) |
| `fail-on-block` | `true` | Fail the step when the verdict is BLOCK |
| `token` | `${{ github.token }}` | Token for the comment and the release download |

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
  contents: read          # always: checkout and the release download
  pull-requests: write    # only when comment is true
  security-events: write  # only when sarif is true
```

The SARIF upload step is `continue-on-error`, so a repository without code
scanning enabled still gets the comment and the job status. On a private
repository, code scanning needs GitHub Advanced Security.

## One comment, edited in place

The Markdown summary starts with the marker `<!-- locrin-report -->` on its own
first line. The action looks for the first pull-request comment whose body starts
with that marker and edits it, so a pull request never accumulates a wall of
Locrin comments. A new comment is created only when no marked comment exists.

## Examples

See `action/examples/`:

- `pull-request.yml`: the gate on every pull request, with comment and SARIF.
- `deploy.yml`: the deployment gate using `since` against the last deploy tag.
- `monorepo.yml`: a matrix over several `path` values in one repository.

## Notes

- `version: local` skips the download entirely and uses whatever `locrin` is on
  PATH. This is what the repository's own `action-smoke` job uses.
- Supported runners: Linux x64, macOS arm64, macOS x64, Windows x64.
- Checksums are verified against `SHA256SUMS` from the same release before the
  archive is unpacked.
