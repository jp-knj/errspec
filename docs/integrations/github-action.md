# Mutation testing in GitHub Actions

The repository's composite Action calls the same `@errspec/core` native API as
local execution. It fetches a missing Git base, restores the dedicated mutation
cache, writes reports and a job summary, emits native diff annotations, and saves
history with `always()` even when the quality gate or baseline fails.

Packages are not published. Install verified matching tarballs in your project,
or pass `native-binary` pointing to an extracted, verified distribution binary.
The Action never falls back to an npm download or builds Rust in the consumer.
Node.js 24 and the project's supported test runner must already be installed.
Check out this repository at a reviewed revision alongside the project, then use
the local Action:

```yaml
permissions:
  contents: read
steps:
  - uses: actions/checkout@v4
  - uses: actions/setup-node@v4
    with:
      node-version: 24
  - run: npm ci --ignore-scripts
  # Check out the errspec Action at a reviewed revision into .tools/errspec.
  # Obtain the matching native artifact and verify its SHA-256 manifest first.
  - uses: ./.tools/errspec
    with:
      native-binary: .tools/native/bin/errspec
      options: '{"include":["src/**"],"policy":{"timeBudget":300,"failOnSurvived":true}}'
  - uses: actions/upload-artifact@v4
    if: always()
    with:
      name: mutation-report
      path: .errspec/action-report
```

An installed `@errspec/cli` is discovered from the selected project when
`native-binary` is absent. The Action source itself must be checked out; the
native artifact alone is not a composite Action. No npm publication, release
creation, repository visibility change or production deployment is performed.

| Input | Behavior |
| --- | --- |
| `working-directory` | Project root, relative to the checkout; defaults to `.` |
| `native-binary` | Explicit executable, relative to the project or absolute; otherwise use installed `@errspec/cli` |
| `since` | Explicit Git base; default is the PR's base SHA, or a full run outside PRs |
| `options` | JSON object using the mutation options schema; `root` and `since` use dedicated inputs |
| `config-file` | Explicit mutation configuration, relative to the project; otherwise use normal discovery |
| `cache` | Restore/save and native cache enablement; defaults to `true` |
| `cache-directory` | Dedicated mutation cache; defaults to `.errspec/mutation-cache` under the project |
| `report-directory` | Defaults to `.errspec/action-report` under the project |

Explicit Action-controlled root, base and cache properties override mutation
configuration. Other option precedence is the public native API's normal
explicit-options-over-configuration behavior. Native exit codes are preserved:
0 success, 1 quality failure, 2 configuration/baseline/incomplete execution, and
130/143 interruption. `exit-code`, `run-complete`, and `report-directory` are
Action outputs. Native JSON, MTE JSON, agent JSON and offline HTML are written
before returning a quality failure. Test progress remains on stderr.

Cache keys are unique per workflow run/attempt. Restore searches the current ref
first and then the PR base branch; save uses the prepared key after a failed run.
Only the dedicated mutation history is cached, not error-contract snapshots or
reports. Restoring history does not establish that it is reusable: native source,
test, dependency, configuration, plugin and environment fingerprints still apply.
Moving checkouts requires opting into `cacheRelocation` and satisfying its
path-independence contract. Changes in `GITHUB_RUN_ID`, `GITHUB_WORKSPACE` or other
environment values can conservatively invalidate results; the Action does not
hide user-observable environment values to force reuse.

The Action's cache service steps require GitHub Actions. Local acceptance invokes
the same preparation/run scripts with real shallow Git checkouts, native reports,
failed gates and copied history. Those checks do not substitute for a successful
live workflow on each supported distribution platform.

The [Distribution workflow](../../.github/workflows/distribution.yml) now invokes
`uses: ./` on Linux x64 and macOS ARM64/Intel. It deliberately fails a completed
quality gate, verifies composite outputs and saved reports, records cache file
hashes, and removes the local cache. An exact GitHub service restore must reproduce
those bytes. The workflow then strengthens the fixture test and calls the composite
again: the same old survivor must be killed after a current successful baseline.
No GitHub environment values are hidden and cross-step native reuse is not
required. Reports and a compact acceptance record are retained as artifacts.

[Test-only setup and verifiers](../../scripts/action-live-acceptance.mjs) have
[local real-native and negative tests](../../tests/action.test.mjs), including
wrong outputs, cache misses, wrong keys and corrupted content. Local copied-cache
tests validate those helpers, not the GitHub service. The live proof remains
pending until these Distribution jobs actually execute successfully; billing
failures before job startup provide no evidence.

The cache steps use GitHub's [restore action](https://github.com/actions/cache/blob/main/restore/README.md)
and [save action](https://github.com/actions/cache/blob/main/save/README.md).
The Action is a [composite action](https://docs.github.com/en/actions/tutorials/create-actions/create-a-composite-action),
so the caller supplies checkout, Node and project dependencies.
