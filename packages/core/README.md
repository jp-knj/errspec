# @errspec/core

Typed mutation configuration and programmatic execution through errspec's native
planner and verdicts. Requires Node.js 24 or later. Package tarballs are available
as CI artifacts; this package is not yet published to npm.

```js
import { defineConfig, runMutation } from '@errspec/core';

const result = await runMutation(defineConfig({
  include: ['src/**'],
  weakMutation: true,
  policy: { failOnSurvived: true },
}));
console.log(result.report.summary);
process.exitCode = result.exitCode;
```

The native binary receives a versioned typed request, rather than reconstructed
CLI arguments. It uses the same configuration resolution, mutation pipeline,
report and exit codes as `errspec mutate`: 0 for success, 1 for a quality failure,
2 for incomplete execution or invalid configuration, 130/143 for interruption.
Quality/configuration failures resolve with their report. Failure to start the
binary, malformed responses, and process termination without a report reject.

`runMutation(options, { cwd, configFile, signal, binary })` can select a working
directory, explicit configuration file, AbortSignal, or locally built binary.
Absent `binary`, it resolves the installed platform package through
`@errspec/cli`. Cancellation interrupts native execution and its owned workers.

## Configuration

Use one `errspec.mutation.config.ts`, `.mts`, `.mjs`, `.js` or `.json` at the
project root, or select a file with `errspec mutate --config-file <path>`.
Script configurations must default-export a JSON-compatible object:

```js
import { defineConfig } from '@errspec/core';
export default defineConfig({
  include: ['src/**'],
  typecheck: true,
  typecheckProject: 'tsconfig.json',
  policy: { timeBudget: 60, thresholdBreak: 70 },
});
```

Explicit CLI flags or API properties override the file, followed by built-in
defaults. Nested `policy` properties merge individually. Arrays replace rather
than append. `--no-typecheck`, `--no-weak-mutation`, `--no-fail-on-undetected` and
`--no-fail-on-survived` override configured `true`; disabling typechecking also
clears configured compiler paths unless explicitly supplied again. Optional
paths and policy limits accept `null` through the API to clear a configured value.

Configuration paths are relative to the effective project root. A configured
`root` is relative to the configuration file. An explicit CLI/API `root` is
relative to the invoking directory and takes precedence. Explicit CLI/API
compiler/Vitest paths retain their invoking-directory semantics. With no `root`
property, the invoking project root is used. Configuration discovery does not
walk parent directories. More than one default configuration is an error.

The shipped [JSON Schema](schema/mutation.schema.json) is also exported as
`@errspec/core/schema`. Unknown properties, invalid types or policy constraints
fail before tests start, including invalid file values overridden by the CLI.
TypeScript configuration loading uses Node's built-in type stripping; runtime
syntax requiring TypeScript compilation is unsupported. Loading scripts uses
the CLI/API `node` option (or `node` on PATH), before a file can supply its own
execution option. Loader logs go to stderr, with a 10-second deadline and 1 MiB
output limits. Configuration scripts run as local project code.

`selectTests`, `formatReport`, plugin APIs, coverage caches and additional runner
packages are subsequent implementation stages; this package currently exposes
`defineConfig` and `runMutation`.

## Git selection

Use `errspec mutate --since <revision> --diff-unit node` to select mutants whose
AST expression/statement spans overlap changed lines. `--diff-unit scope` selects
the enclosing named function/method or function-valued binding; module-level code
uses the module scope. The default unit is `node`. API/configuration equivalents
are `since` and `diffUnit`. `--only` intersects with this selection.

The base includes staged, unstaged and untracked project files. Paths are rooted
at the package, including inside a monorepo. Changes to tests, removed tests, or
files outside the mutation inventory conservatively select all candidates because
dependency/configuration effects cannot be ruled out. Changed or removed test
files absent from the baseline are warnings in `selection.warnings`; warnings
are separate from quality failures. Invalid Git bases stop before running tests.
A change to Git inputs during execution invalidates the result.

Both unmodified and instrumented baselines still run all selected-project tests.
This stage reduces mutant trials, rather than claiming baseline or result reuse.
A completed diff report describes only its selected mutants; consult
`selection.selected`, `omitted` and `fullRunReason` before interpreting its score.
Exact-input result reuse is available separately with `--cache`; removing tests
invalidates those results. Scope-aware reuse and experimental callgraph selection
are subsequent stages.

## Partial runs and quality gates

Sampling or a time budget may leave selected candidates `Pending`. Such a run
can finish the requested plan with exit 0 when no quality gate is requested, but
its overall `summary.score` remains `null`. `--threshold-break` cannot pass while
that score is unknown, and `--fail-on-undetected` also rejects unexecuted selected
candidates. `--fail-on-survived` continues to check survival specifically. Diff
selection reports omitted candidates separately; its score applies to the selected
request, as shown by `selection`.

## Reusing Vitest sessions

`--reuse-session` / `reuseSession: true` keeps the instrumented Vitest/Vite
context between the probe baseline and mutant trials. The unmodified baseline
still uses its own context. This option is off by default; `--no-reuse-session`
overrides a configured true value. Isolated fork workers, request-specific probes,
test selection, and original timeout values are renewed for each attempt. Only
timeout confirmation receives the extended timeout.

Vite configuration/lifecycle hooks run once for the instrumented session. Test
setup files and isolated test modules run for each attempt. Configurations with
`globalSetup` keep fresh contexts so their setup and teardown retain the existing
per-attempt lifecycle. Versions missing the public discovery-reset API also keep
fresh contexts; reuse is validated with Vitest 5.0.0 and 5.0.5.
`engine.sessionReuse` reports `available`, `fresh_global_setup` or
`fresh_lifecycle_unknown` when requested. Cleanup errors and stale execution
events invalidate the run. Interrupts/deadlines still terminate the owned group.

## Exact-input result cache

`--cache` / `cache: true` reuses stable first-attempt `Killed` and `Survived`
outcomes. It is off by default. Use `--no-cache` to override configuration and
`--cache-dir <directory>` / `cacheDir` to select a dedicated results directory
(default: `.errspec/mutation-cache`, relative to the project root). Add that
directory to `.gitignore`. Cache storage is separate from error-contract snapshots
and cannot overlap the project root or unrelated existing files.

Both current baselines still execute. Reuse requires the same compiled engine,
Node/runtime and environment, execution options, resolved configuration and
provided values, project files, runtime inputs and installed dependency contents,
plus matching candidate, covering and
selected test IDs. Dependency edges are bound even when packages are shared.
Gates are recomputed from the current request. `mutant.reused` and `report.cache`
distinguish cached outcomes from current trials; the baseline remains current.

Unknown versions, malformed records and corrupt files cannot establish a
detection. They trigger fresh verification. Timeouts, runtime errors, retried
outcomes, typechecking and reachability are revalidated. Results are saved
atomically after final input validation, before human acceptance is applied.
Even an all-hit run refreshes source/test discovery at the end. Input changes
invalidate reused evidence; uncertain cache inputs disable reuse instead.

This stage assumes deterministic tests whose inputs are represented by the
project, installed dependencies and acquired runtime files/environment. Keep
cache disabled for uncontrolled external inputs. Node preload hooks or NODE_PATH
that hide dependency acquisition also retain fresh verification. `globalSetup`
and configurations whose resolved values cannot be serialized disable the cache
until their external inputs and lifecycle can be acquired. Persistence errors
remain visible as incomplete execution.

The first cache is deliberately strict within a checkout: position-only edits
keep mutant IDs but invalidate results. Coverage reuse, cross-checkout reuse,
scope-aware invalidation and experimental callgraph selection are subsequent
stages, not provided by this initial cache.
