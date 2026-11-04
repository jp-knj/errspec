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

`selectTests`, `formatReport`, plugin APIs, result caches and additional runner
packages are subsequent implementation stages; this package currently exposes
`defineConfig` and `runMutation`.
