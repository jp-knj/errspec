# Jest native ESM mutation API

This package selects Jest and passes the shared validated request to `@errspec/core`. Native Rust owns candidate generation, scheduling, retry confirmation, cache policy and verdicts. There is no CLI argument reconstruction or independent verdict loop.

Use Node 24+, Jest 30 native ESM and the project's own configuration:

```js
import {runMutation} from "@errspec/jest";
const {exitCode, report} = await runMutation({
  root: process.cwd(),
  runnerConfig: "jest.config.mjs",
  include: ["src/**/*.ts"],
  concurrency: 2,
});
console.log(report.summary);
process.exitCode = exitCode;
```

The embedded adapter starts fresh owned Jest processes and preserves ESM transforms and their options. When `transform: {}` is used, Node strips TypeScript types. CommonJS and Jest multi-project configurations are outside this adapter's contract. `runner` is fixed to `jest`; conflicting options are rejected. Unknown external inputs cause cache revalidation. Install from the staged local tarballs with core, CLI and the native platform package; these development commands do not publish packages.
