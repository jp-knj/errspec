# Vitest mutation API

This package selects Vitest and calls `@errspec/core` with the same validated native request used by the CLI. It does not assemble CLI arguments or implement a second planning/verdict loop. Install it from the staged local tarballs together with core, CLI and the native platform package; registry publication is a separate release step.

Use Node 24+ and the project's own Vitest 5 installation:

```js
import {runMutation} from "@errspec/vitest";
const {exitCode, report} = await runMutation({
  root: process.cwd(),
  runnerConfig: "vitest.config.ts",
  include: ["src/**/*.ts"],
  weakMutation: true,
});
console.log(report.summary);
process.exitCode = exitCode;
```

All shared core options and transport cancellation apply. `runner` is fixed to `vitest`; conflicting values are rejected. `runnerConfig`, `runnerProject` and `runnerPackage` select the installed runner without changing the original source or snapshots. Configuration files remain separate `errspec.mutation.config.*` files.
