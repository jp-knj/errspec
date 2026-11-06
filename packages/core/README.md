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

`selectTests` and additional runner packages are subsequent implementation stages.
`formatReport` shares the native presentation with the CLI; `defineMutator`,
`defineIgnorer` and `definePlugin` define module plugins.

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

Without caching, both baselines run all selected-project tests. The unmodified
baseline always runs; `--cache` can reuse the instrumented probe separately.
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

## Exact-input result and coverage caches

`--cache` / `cache: true` reuses stable first-attempt `Killed` and `Survived`
outcomes. It is off by default. Use `--no-cache` to override configuration and
`--cache-dir <directory>` / `cacheDir` to select a dedicated results directory
(default: `.errspec/mutation-cache`, relative to the project root). Add that
directory to `.gitignore`. Cache storage is separate from error-contract snapshots
and cannot overlap the project root or unrelated existing files.

The ordinary baseline still executes. Reuse requires the same compiled engine,
Node/runtime and environment, execution options, resolved configuration and
provided values, project files, runtime inputs and installed dependency contents,
plus matching candidate, covering and
selected test IDs. Dependency edges are bound even when packages are shared.
Gates are recomputed from the current request. `mutant.reused` and `report.cache`
distinguish cached outcomes from current trials.

Compilation values are acquired from the resolved Vite root and execution
environments as well as Vitest's runtime-injected defines. Dynamic `define`,
resolution, compiler, environment and optimizer settings must still match when
the configuration file itself is unchanged. Known filesystem locations use the
optional relocation identity; define expressions and runtime values stay raw.

Unknown or duplicate compilation plugins, opaque asset matchers, runtime define
scripts and unserializable compiler options keep verification fresh. CSS disabled
by Vitest uses its serialized test policy; CSS processing with callbacks also
keeps fresh verification. The disabled-cache reason remains visible. Custom Vite
transform/resolve/load closures need acquired inputs before they can reuse results.

Probe observations use a separate `coverage-v1.json` document. Only after the
current ordinary baseline passes, matching input fingerprints, candidate references
and complete test registration/statuses allow reuse. `cache.coverageReused` marks
this explicitly. Current task IDs and the longer of current/previous timings are
used for selection and deadlines. Changed or removed tests refresh observations.
`--full-dry-run` / `fullDryRun: true` always reruns the full instrumented probe,
independently of result reuse. `--no-full-dry-run` overrides configured true.

Unknown versions, malformed records and corrupt files cannot establish a
detection. They trigger fresh verification. Timeouts, runtime errors, retried
outcomes and typechecking are revalidated. Coverage with incompatible or malformed
observations refreshes the full probe. Results and coverage are saved
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

By default, position-only edits keep mutant IDs but invalidate results and
coverage. Experimental callgraph selection remains a subsequent stage.

### Reusing unchanged functions after edits

`--cache --cache-scope` / `cache: true, cacheScope: true` enables conservative
function-level result reuse. This is off by default; `--no-cache-scope` overrides
configured true. Enable it for deterministic, isolated test files whose behavior
does not depend on source text, source positions or stack-trace locations.
Source reflection is outside this mode's contract.

Every source edit still refreshes the full instrumented probe. Syntax without
positions identifies unchanged function bodies. The native planner compares the
test files that entered changed functions both before and after the edit. It
reuses an unchanged function's assertion outcome only when those files are
disjoint from all of the mutant's current covering test files, and its covering
and selected test IDs still match. Different tests in one file are treated as
sharing state. Calls connected only through test code therefore still revalidate.
Unchanged position-only edits can reuse results across all functions.

Changed scopes with missing/excluded entry guards or opaque calls, changed scope
inventories and overlapping file cohorts revalidate. Compiler comments currently require the raw
identity of their entire file, so even moving a commented file's code revalidates;
comment attachment semantics are not inferred. Module initialization/signatures,
anonymous default declarations without a function owner, test/helper files,
dependencies, resolved compilation/plugins, environment and
other execution inputs remain bound. Unknown compilation inputs disable both
forms of cache reuse. New timeouts, retries and typechecks use the ordinary
verification rules.

`cache.scope` identifies the mode, `cache.scopeReused` counts outcomes recovered
from the scoped document, and `cache.reused` includes exact and scope hits.
`cache.invalidated` can be true when exact inputs changed but a subset of scoped
outcomes was retained. `scope-results-v1.json` is separate from strict results and
coverage. Both paths check actual raw inputs again before saving. Unselected
scoped records are retained only when their complete acquired context is
unchanged; an edited context may require another invocation to fill its cache.

### Reusing a cache in another checkout

`--cache --cache-relocation` / `cache: true, cacheRelocation: true` allows the
project root to move. This is off by default. Enable it only when deterministic
tests and dependencies have the same meaning after relocation: their assertions
and behavior must not depend on the absolute project location, `cwd`, `PWD` or
`INIT_CWD`. `--no-cache-relocation` overrides configured true. `cache.relocation`
marks this mode; switching modes invalidates stored evidence.

Owned project paths, dependency edges under the project, known runner setup paths
and the project portion of invocation/runtime locations use tagged relative
identities. Actual current files and installed dependency contents are still
hashed, including external packages and resolved dependency edges. External
locations retain their absolute identity. Other environment values, provided
context, configuration values and file contents are compared exactly; a value
containing an absolute path there can deliberately prevent cross-checkout reuse.
This mode does not rewrite project files, runtime paths, user configuration or
test values. It changes only fingerprint identities for the declared locations.

Copy the dedicated cache directory alongside an otherwise identical checkout.
The current ordinary baseline must still pass and registration must match before
probe/results reuse. Current task IDs are rebound, and final input validation
still detects changes. Dependency/test/configuration/environment changes or
deleted killing checks trigger fresh verification, including after relocation.
Inputs such as a different CI run ID remain bound and may prevent reuse; storing
the cache alone does not establish that two environments are interchangeable.

## Experimental static-call reuse

`--cache --cache-scope --experimental-callgraph` / `experimentalCallgraph: true`
can additionally reuse old survivors when neither the old nor current source-call
graph connects their function to a changed function in either direction. The
setting requires both cache modes, defaults off, and has an explicit
`--no-experimental-callgraph` override. Switching it off invalidates its evidence.

This is an approximation: values can flow between otherwise disconnected calls
through callers, tests or shared state. An old survivor can therefore remain a
survivor even when a fresh run would kill it. Old detections are always
revalidated when ordinary scope evidence requires it. A warning,
`cache.experimentalCallgraph` and `cache.callgraphReused` expose the mode and its
extra reused outcomes.

Calls use acquired lexical owners, named/namespace/default import bindings,
actual Vite import resolution and method receivers. Unknown or ambiguous calls,
parameter shadowing, missing entry evidence and incompatible graphs keep fresh
verification. Callers sharing a callee alone are not connected by a directed path.
Coverage still refreshes fully after edits; this option only prunes result trials.

## Sharded execution

`--shard 1/4` selects one deterministic partition of the requested candidates.
The API/configuration form is `shard: { index: 1, total: 4 }`; indices start at
one. Explicit CLI/API selection replaces the configured shard, API `null` clears
it, and `--shard 1/1` includes the whole requested inventory.

Partitioning uses the stable mutant key, after Git and `only` filters. Iteration
order and position-only edits do not alter assignments. The union of all shards
contains each requested candidate once. `report.shard` reports the algorithm,
eligible, selected and omitted counts. Scores and quality gates apply to that
shard; they do not establish a combined full-project result. Pending selected
candidates still leave the score unknown and cannot pass a full-score gate.

Baselines and cached probe observations cover the whole selected runner project.
Typechecking and mutant trials use only the shard. Compatible results from
sequential shards can accumulate in the same cache and support a later unsharded
repeat. Concurrent writes remain atomic but may lose cache entries, requiring
fresh verification. Sampling and time budgets apply separately within each shard
invocation. Sharding does not start parallel workers by itself.

## Parallel execution

`errspec mutate -j 2` / `--concurrency 2` runs independent mutant trials in
parallel. The API/configuration property is `concurrency`; it accepts integers
from 1 to 64 and defaults to 1. Explicit CLI/API values override the file.
`engine.parallel` reports the requested limit and number of trial workers started
when concurrency exceeds one. Fully cached or already exhausted runs start zero
trial workers. Baselines and typechecking still precede the trial pool.

Each additional worker owns its runner context, probe identity and process group.
It reruns the full instrumented probe and validates test registration, statuses,
reachability and infection against the primary probe. Mismatches cancel the run
with an unknown score. Opt-in session reuse stays within each worker; test modules
remain isolated between attempts. Timeout confirmation restarts and revalidates
only the affected worker. Any worker failure cancels active peers, while interrupts
and unexpected owner exits terminate every owned group and ordinary descendants.

Results retain inventory order regardless of completion order. Sampling applies
before scheduling. The time budget is shared across this invocation: in-flight
attempts finish, while candidates not started by the deadline remain Pending.
Shards have separate budgets. Cached outcomes use the same evidence rules across
serial and parallel runs. Every worker's runtime inputs are merged and validated
before saving; inconsistent inputs disable cold persistence and invalidate reused
evidence. Parallel tests must support simultaneous external side effects, such as
separate database records or ports. This option does not isolate external services.

## Worker ownership and orphan recovery

Mutation runners use a native supervisor as their process-group leader. The
native owner retains its child handle until group termination. If the owner
exits unexpectedly, the supervisor detects its changed kernel parent and ends
its own group, including blocked Node workers and ordinary descendants.

Versioned ownership records and capability-authenticated Unix endpoints live
under a private `/tmp/errspec-workers-v1-<uid>` directory. Startup recovery asks a
live supervisor to verify its owner and group before terminating itself. It never
signals a PID from an old record. Active runs, mismatched identities and tokens
are left alone. Scanning is bounded to three seconds and 256 records; unresolved
records are reported in optional `workers` statistics and warnings.

When Unix endpoints are prohibited, automatic parent monitoring remains active.
Set `ERRSPEC_WORKER_ENDPOINT=0` to select that mode explicitly;
`workers.endpointUnavailable` identifies it. Immediate recovery through an
endpoint is unavailable in that mode. Normal interrupts and deadlines still end
the currently owned group. Orphan cleanup removes only recognized temporary
files and leaves unexpected files alone. Workers deliberately detached into a
different group remain outside this ownership contract.

## Explicit source exclusions

Exclude an intentional case without changing tests:

```ts
// errspec-disable-next-line BooleanLiteral: specified by the external protocol
export const enabled = true;
```

`errspec-disable-next-line`, `errspec-disable-line`, and region directives
`errspec-disable` / `errspec-enable` accept comma- or space-separated mutator
names. Omitting names, or specifying `all`, selects every family. A colon
introduces an optional reason. The `mutator-` aliases and Stryker's
`Stryker disable [next-line] <names>` / `Stryker restore <names>` are supported.
Region directives apply after the comment, until a matching enable/restore.
Line directives take precedence; next-line means the physical line after the
comment ends, including for block comments. Restore only the named families
to keep other region exclusions active.

Only lexical comments count. Directive text inside strings, regular expressions,
template text or JSX text cannot suppress candidates. Malformed directives are
ignored. Suppressed candidates retain their IDs, appear as `Ignored` with
`ignored: "disabled[: reason]"`, run no trials and stay outside the score and
gates. Removing a directive restores verification and invalidates exact-input
cached evidence. Existing built-in safety exclusions keep their own reason.

## Logging-only exclusions

`--arid` / `arid: true` marks logging-only syntax `Ignored` with
`ignored: "arid: logging-only code"`. It is off by default to preserve existing
mutation runs. `--no-arid` overrides configured true. This is a naming heuristic
for intentional log output, rather than a proof of equivalent behavior: calls,
their arguments and logging-only conditions may have effects that matter to you.
Keep it disabled when those effects belong in your test contract.

The default callee patterns are `console.*`, `logger.*`, `log.*`, `*.debug`,
`*.trace` and `debug`. `--arid-callee <pattern>` (repeatable) or
`aridCallees: [...]` replaces that set; it does not enable the heuristic by itself.
An empty array matches no calls; API `null` restores the built-in patterns.
Patterns support exact dotted names, a trailing `.*` for a named receiver,
or a leading `*.` for a property suffix. `this.logger` uses the `logger` path.
Optional access, parentheses, non-null assertions and awaited calls are supported.
Computed members and casts are kept for verification.

Calls and blocks/ifs containing only logging or empty statements are excluded.
Mixed blocks, return statements, declarations and nested callback bodies retain
their independent behavior checks. Empty function bodies are not classified as
logging-only. Explicit source comments and built-in safety exclusions keep their
own reasons. Candidates retain their IDs; changing the enablement or patterns
invalidates cached outcomes and observations.

## Report presentation

The existing `--format json` remains errspec's native report on stdout. Additional
reporters use `errspec mutate --reporter agent,json,html --report-dir reports`:

- `text` describes verdicts, reasons and warnings on stdout.
- `json` saves `mutation.json`, conforming to mutation-testing-elements schema v2.
- `html` saves `index.html`, an offline viewer with embedded source, verdicts and
  status/file filters. It loads no scripts, fonts or other resources from a network.
- `agent` writes one JSON document containing gaps (`notDetected`, `notInfected`,
  `notReached`), enclosing function source, reaching tests and suggested next steps.
- `simplify` lists covered removal mutants that survive. These are review
  candidates; a passing test suite does not establish that deletion is safe.
- `annotations` writes GitHub warning commands for selected diff gaps and critical
  gaps. In GitHub Actions, default text and other explicit reporters also emit annotations except
  `agent`, whose stdout remains a single JSON document.

Choose at most one stdout reporter; combine it with `json` and/or `html`.
File reporters default to `<root>/.errspec/report`. A configured `reportDir` is
relative to the effective project root; an explicit CLI path is relative to the
invoking directory. Configuration uses `reporters: ['agent', 'html']`; explicit
`--reporter` replaces that array. File paths are announced on stderr. Reporter
failures exit 2; completed runs preserve their existing quality exit code.
Reports preserve `Pending`, an unknown score, reasons, acceptance and quality
gates. The MTE schema requires integer thresholds, so fractional display thresholds
are rounded up; the original policy and its native verdict remain in `errspec`.

```js
import { formatReport, runMutation } from '@errspec/core';
import { writeFile } from 'node:fs/promises';
const { report } = await runMutation({ include: ['src/**'] });
const json = await formatReport(report, { reporter: 'json', root: process.cwd() });
await writeFile('mutation.json', json);
```

`formatReport(report, { reporter, root, sources }, { binary, cwd })` returns a
Promise of one string, using the CLI's Rust formatter. It runs no tests and writes
no files. `runMutation` returns the native report; use `formatReport` to present
it. For retained reports, supply original source snapshots through `sources`,
keyed by relative report path, if the files have changed or been removed.
The formatter verifies each original mutation span and line against that snapshot;
it rejects mismatches instead of attaching stale evidence to changed code. This
span check does not establish that unrelated source still matches an earlier run.
Report file names must be normalized root-relative paths (at most 4096 bytes).
Filesystem targets must resolve inside the canonical project root and be regular
files; acquired source content is bounded to 64 MiB per formatting request. Explicit
`sources` snapshots follow the same filename policy and need no filesystem reads.
Source acquisition for agent/simplify uses AST function boundaries rather than
line-based guesses. The upstream MTE schema is exported as
`@errspec/core/report-schema`, with its Apache-2.0 license in the schema directory.


Observed child processes are reported only when a worker actually launches a command
that names a project script. Public attribution contains the test/module, executable
name, launch method and possible local source files; raw arguments, environment and
stdio are excluded. Async `spawn`/`fork`/`exec` use the spawn tracing diagnostics channels available in
the validated Node 24.22 runtime; early Node 24 minors without those publishers
are not covered by this async observation.
Node has no corresponding synchronous channel, so the three synchronous exports are
wrapped while the test module runs and restored afterward. This does not instrument
code inside the child or change native quality gates.

Literal runtime imports (including side effects), re-exports, `require` and `import`
expressions extend script entries through relative project script dependencies.
Type-only imports and text inside literals do not add edges. Computed imports,
package/alias resolution, shell expansion and scripts outside the project are not
inferred. A reported file may be loaded by the command; attribution does not prove
that a particular function ran. Agent/MTE/HTML distinguish this possibility, and
simplify excludes those files. A command without a discovered project script does
not create a generic warning. No warning establishes that every subprocess was
observable or that child execution was safely mutation-tested.

## Custom mutation plugins

Use `plugins: ["./mutation-plugin.mjs", "package-name"]` in configuration/API,
or repeat `--plugin <module-or-package>` in the CLI. Relative modules resolve from
the project root; package names resolve from that project's dependencies. Each
module exports a mutator, ignorer, plugin bundle, or an array as its default.
Node 24 also supports TypeScript modules with erasable syntax.

```js
import { defineMutator, defineIgnorer, definePlugin } from "@errspec/core";

export default definePlugin({
  name: "project-rules",
  cache: "deterministic",
  mutators: [defineMutator({
    name: "ReplaceConstant",
    visit(node, context) {
      if (node.kind === "NumericLiteral" && node.text === "2") {
        return [{ replacement: context.settings?.replacement ?? "3" }];
      }
    },
  })],
  ignorers: [defineIgnorer({
    name: "Invariant",
    shouldIgnore(node) {
      if (node.kind === "CallExpression" && node.text.startsWith("invariant(")) {
        return "assertion helper";
      }
    },
  })],
  aridCallees: ["metrics.*"],
});
```

Pass JSON settings as `plugins: [{ module: "./mutation-plugin.mjs", settings:
{ replacement: "4" } }]`. Callbacks stay in the imported module; execution options
remain JSON values. The three definition helpers preserve callbacks and provide
types. `PluginContext.settings` is a JSON value: narrow it when using TypeScript.

Nodes expose TypeScript syntax `kind`, `name`, `text`, byte `start`/`end`, and
parent/child indexes into `context.nodes`. `context.parent` returns the parent
node; `context.text({start,end})` reads UTF-8 byte ranges. A mutator returns edits
inside its visited node, optionally with `range: {start,end}`. Visit the complete
runtime expression when editing a property token. Rust validates anchors and
ranges, reparses the changed file, chooses safe switch placement and generates
stable IDs. Block anchors require block replacements. Unsupported anchors and
invalid syntax fail with the mutator name; an unsafe switch is reported Ignored.

`excludeMutator: ["ReplaceConstant"]` or `--exclude-mutator ReplaceConstant`
disables a custom family. Names must be unique and cannot shadow built-ins.
Ignorers apply to a node's descendants and report `<name>: <reason>`.
Built-in safety rules, explicit disable/Stryker comments and arid rules retain
their earlier reasons. Plugin `aridCallees` extend the configured patterns when
`arid` is enabled. Generation does not change existing built-in IDs.

Caching is conservative by default. `cache: "deterministic"` promises that the
plugin depends only on its imported module closure, JSON settings, environment
and acquired files. Set it on a bundle to cover its members. Module imports,
package manifests and their contents are acquired automatically, including
imports performed by async visitors. Declare other files with `inputs: [...]`
or read them with `context.readInput(path)`; these paths resolve from the plugin
module. Generation acquires these inputs before baseline tests execute and rejects
changes during the run. Changes to plugin code, dependencies, settings or acquired data invalidate
both result and reachability reuse. Omit the promise for plugins depending on
time, randomness, services or undeclared file reads; those runs verify afresh.
