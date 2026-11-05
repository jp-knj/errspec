---
name: errspec
description: Set up and run errspec mutation testing for a TypeScript or JavaScript project, choose its runner and configuration, and interpret native or agent reports locally and in CI.
license: MIT
---

# Set up errspec

Use the project's installed `errspec` executable or a verified matching native
artifact. The registry packages are not published; do not assume an npm download
is available. Install the local CLI/core/native tarballs as the project's package
manager supports, preserving its lockfile. Rust is not required in the consumer.
Mutation execution and the core API require Node.js 24 or later.

Start with the project's ordinary test command. A baseline failure is incomplete
verification, not a mutation quality failure. Select a small source area first:

```sh
errspec mutate --include 'src/core/**' --reporter agent
```

Choose `--runner vitest`, `vite-plus`, `jest` or `playwright` when auto detection
is ambiguous. Vitest requires version 5; Jest supports version 30 native ESM.
For browser code, use the shipped `errspec-playwright` skill. Read
[runner settings](references/runners.md) when selecting configs, projects or a
package installed outside the project.

Persistent settings belong in `errspec.mutation.config.{ts,mts,mjs,js,json}`,
separate from the `errspec.json` error-contract configuration:

```json
{
  "include": ["src/**"],
  "cache": true,
  "policy": {"timeBudget": 120, "failOnSurvived": true}
}
```

CLI/API explicit properties override configuration. Arrays replace; policy
properties merge. `--no-cache`, `--no-typecheck` and other negative flags override
configured true. More than one default mutation config is an error; select one
with `--config-file`. See [configuration and CI](references/configuration.md).

Use `--since <base>` for Git diff selection and `--cache` to retain results.
Caches remain subject to source/test/dependency/configuration/environment checks.
Do not describe restoration alone as reused evidence. Cache relocation and scope
reuse are explicit contracts, not defaults. Ignore `.errspec/` generated data in
Git; commit reasoned `errspec.accepted.json` decisions when they are part of the
requested work.

Interpret the report before changing tests:

| Result | Evidence and next step |
| --- | --- |
| `notDetected` | Reaching tests pass under the mutation. Assert the changed observable result. |
| `notInfected` | Reaching inputs never change the value. Add a separating boundary/input. |
| `notReached` | The worker did not observe execution. Add a reaching test after checking warnings. |
| Child-process warning | A spawned command may load the file outside instrumentation. Prefer an in-process assertion. |
| `Pending` / incomplete | Verification remains outstanding. An unknown score is not 100%. |

`--reporter agent` is one JSON document on stdout; progress is on stderr.
`--format json` is the native JSON report on stdout. `--reporter json,html` writes
MTE JSON and offline HTML under `.errspec/report` or `--report-dir`; these are
separate presentation choices. Use `errspec-test-loop` to work through gaps.

Exit 0 means the requested run passed; 1 means a quality gate failed; 2 means
configuration, baseline or incomplete execution. Interrupts use 130/143.
A passing selected subset does not establish a full-project score or correctness.
