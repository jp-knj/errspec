# @errspec/testkit (unpublished)

A small ESM package that records failure-scenario observations for the Rust CLI
from Bun test and Vitest. The runtime is ordinary JavaScript with declarations in
`index.d.mts` and no external runtime dependencies. `index.ts` is the canonical
implementation and can also be copied on its own; the package entry remains `index.mjs`.
Node standard modules are required, so browser tests are outside this API's scope.

This repository uses relative imports. Another project can copy the directory and
add it as a local dependency. For example, after placing it in
`vendor/errspec-testkit`, add
`"devDependencies": { "@errspec/testkit": "file:./vendor/errspec-testkit" }`
to package.json and install with the project's package manager. This refers to a
local copy, not a published registry package. Prepared tarballs are another local
installation option described in the [distribution guide](../../docs/engineering/guidelines/distribution.md).

Projects that cannot retain JavaScript files, or want a helper beside their tests
without an added dependency, can copy `index.ts` alone. It includes declarations;
import it with `import { scenarioTest } from "./errspec-testkit"`. It works with
strict typechecking and Bun/Vitest. Node module types require `@types/node` or
`@types/bun`. The same tests run against `index.ts` and `index.mjs` to check their
behavior. Record the source commit when copying: manual changes can obscure which
upstream version the copy represents.

Maintainers edit `index.ts`, then run `node scripts/build-testkit.mjs` from the
repository to regenerate `index.mjs` and `index.d.mts`. The script reuses the
pinned TypeScript 6.0.3 development compiler from `packages/analysis-driver`;
install those dependencies with `npm ci --prefix packages/analysis-driver` first.
`node scripts/build-testkit.mjs --check` verifies the committed artifacts without
rewriting them. Check and Distribution CI enforce this before testing or packing.
Consumers of either the package or the copied TypeScript file require no build.

```ts
import { test } from "bun:test"; // or vitest
import { scenarioTest } from "@errspec/testkit";

scenarioTest(test, import.meta.url, {
  contract: "gateway", id: "offline", tag: "Offline",
}, async (s) => {
  const fetchUser = s.fault("network", async () => { throw new Error("offline"); });
  let message = "";
  try { await fetchUser(); } catch (error) { message = (error as Error).message; }
  s.check("message", message, "offline");
});
```

This short example demonstrates the stub. In application tests, inject `fetchUser`
into the operation and observe its response or side effects.

## Faults in directly imported dependencies

Some operations import dependencies instead of receiving them as arguments.
Runner module mocks can supply faults without rewriting production code for
dependency injection. Let the mock forward through a replaceable reference; each
scenario assigns its own `s.fault` wrapper to that reference.

Bun:

```ts
import { mock, test } from "bun:test";
import { scenarioTest } from "@errspec/testkit";
import { notify } from "../src/notify"; // imports deliver from "./mailer"

const mailer = { deliver: async (_to: string) => ({ accepted: true }) };
mock.module("../src/mailer", () => ({ deliver: (to: string) => mailer.deliver(to) }));

scenarioTest(test, import.meta.url, { contract: "notify", id: "offline", tag: "retry_later" }, async (s) => {
  mailer.deliver = s.fault("mailer", async () => { throw new Error("offline"); });
  s.check("result", await notify("user@example.com"), { status: "retry_later" });
});
```

Vitest:

```ts
import { test, vi } from "vitest";
import { scenarioTest } from "@errspec/testkit";
import { notify } from "../src/notify";

// vi.mock is hoisted before imports, so create the reference with vi.hoisted.
const mailer = vi.hoisted(() => ({ deliver: async (_to: string) => ({ accepted: true }) }));
vi.mock("../src/mailer", () => ({ deliver: (to: string) => mailer.deliver(to) }));
// Use the same scenarioTest body as the Bun example.
```

- Do not call `s.fault` while constructing the module mock: the mock is evaluated
  before the scenario, when `s` does not exist.
- Reassign the reference at each scenario's start so earlier faults do not leak.
- If the mock is ineffective, the real dependency runs and the required fault
  remains uncalled. The CLI fails with EC009.
- [examples/module-mock](../../examples/module-mock) contains a complete example.
  Run `node scripts/demo-module-mock.mjs` for both runners.

## API

### State preservation under single faults

`checkFailureAtomicity({ setup, faults, isFailure, only? })` observes ordinary
dependency calls, then injects one fault at each call ordinal. `setup` returns an
independent fixture for each execution with `run(scope)`, `snapshot()`, and optional
`dispose()`. `scope.wrap(dependency, implementation)` wraps an async dependency,
preserving arguments and `this`; the selected call runs the fault function instead
of the real implementation.

Each fault has a unique `id`, a `dependency`, and an `inject` function returning a
valid failure value or throwing. `isFailure(result)` judges the operation's return
value. Applications handle injected exceptions and convert them into results;
an exception escaping the operation fails the check.

For failure values, compare cloned before/after snapshots. Shared references do
not hide mutations; a rollback restoring the same state passes. Snapshot the
business state to preserve, excluding logs, timestamps, and secrets.

`baseline.calls` reports ordinary calls by dependency. Each case has `fault`,
`status`, `before`, `after`, and `result`. `preserved` means state equality after a
failure, `recovered` means a successful result after the fault, and `violated`
includes state change, missing injection, exceptions, or cleanup failure. Execute
all cases before throwing `FailureAtomicityError`; its `report` retains evidence.
Pass a case's `only: { fault, call }` values to replay that one case.

`recovered` records a success value, not proof of correct recovery. Existing tests
must also check successful outputs/state to detect swallowed errors. An ordinary
baseline failure, unused registered dependency, empty/duplicate faults, or
unfinished dependency call cannot pass. `dispose()` runs on failure too. This API
does not stop background activity: operations must await dependencies and fixtures
must isolate external state.

The API does not infer arbitrary fault values or business expectations from types.
It explores single faults at the ordinary baseline's observed call ordinals.
Existing tests judge recovery and snapshot adequacy. Reports hold JavaScript values
and are not guaranteed to be JSON-serializable.

### Scenario execution evidence

- `scenarioTest(test, fileURL, { contract, id, tag, withoutFaults? }, body)` registers
  a top-level or nested `describe` test. Await all required async work in its body.
  `withoutFaults: true` explicitly declares an input-only scenario and pairs with
  configuration `observe.faults: []`. With this flag, calling `fault` fails; without
  it, a scenario with zero faults fails.
- `scenarioTestName(contract, id)` builds the stable name used by configuration.
- Optional `description` appends one space and a nonempty single-line explanation
  to the test name. The CLI matches the stable identity without that explanation;
  observations retain the runner's corresponding full name.
- `scope.fault(id, fn)` returns an async wrapper preserving arguments and `this`,
  recording call/completion counts. Both resolve and reject count as completion.
- `scope.check(id, actual, expected)` performs `deepStrictEqual`. It rethrows and
  internally retains comparison failure.

Fault and assertion IDs are separately unique nonempty strings within a scenario;
the same spelling can appear in both namespaces. Repeated/retried execution of one
scenario is ambiguous to the Rust verifier and is rejected. Distinct scenario IDs
can use `test.concurrent`.

## Recording lifecycle

The CLI supplies `ERRSPEC_OBSERVATIONS` and `ERRSPEC_RUN_ID` to its child process.
Each scenario creates a UUID file with starting state, then records final state,
fault counts, and assertions through temporary-file rename. Parallel runs do not
mix JSON lines. Setting only one variable is a configuration error. With neither,
the helper behaves as an ordinary unit test and creates no evidence files.

Do not hardcode these variables into project settings; old observations cannot be
submitted as current execution. Scope operations after test completion fail and
mark evidence incomplete. Activity after process exit or after CLI evidence
acquisition cannot be traced.

## Meaning and trust

Fault completion establishes that a developer-selected function ran and returned
or threw. This helper does not interpret arbitrary Result libraries or prove that
the returned value belongs to the intended failure category. Comparisons execute,
but expected values and observed-state adequacy still need review. Evidence comes
from trusted tests; it is not proof against malicious test-code tampering.
