# Embedded mutation driver

The Rust binary embeds `dist/driver.mjs` and `setup.mjs`. End users need Node 24+ and their own supported runner installation. They do not build or install this private package.

`driver.mjs` owns runner sessions and the versioned NDJSON protocol. `emitter.mjs` applies the native generator's byte ranges with MagicString. `setup.mjs` records worker hits and test metadata before user setup files execute. The Rust CLI owns judgments, deadlines, temporary files, and the process group.

`source-inventory.mjs` shares raw source registration, per-source plugin generation, and ancestor manifest tracking across runner baseline acquisition. Each runner retains its own enumeration, exclusions, acquisition order, and final input validation. Plugin callbacks still run once per source before the baseline.

After editing driver or emitter source, rebuild the committed bundle and notices with Bun 1.4.4:

```sh
npm ci --ignore-scripts --no-audit --no-fund --prefix packages/mutation-driver
node scripts/build-mutation-driver.mjs
node scripts/build-mutation-driver.mjs --check
cargo build --locked
node --test tests/mutation-emitter.test.mjs
node scripts/demo-mutation.mjs
```

Runner libraries are imported from the selected installation and are never bundled. Vitest 5 and Vite+'s bundled Vitest use the existing isolated worker adapter. Jest 30 native ESM uses fresh owned processes, wrapping the project's ESM transforms and recording its runner events. Native TypeScript stripping is available when the project uses `transform: {}`. The temporary state file supports restarting the driver after a deadline. Persistent caches are opt-in; the additional adapters conservatively revalidate when external runtime inputs are unknown.

Vitest, Jest and Playwright Node workers share `child-process.mjs` acquisition and the same source attribution path. Actual launches contribute redacted advisory facts; native Rust still determines every verdict and gate. Jest observes custom environment setup through teardown and restores the observation hooks between environments. Playwright's automatic worker fixture covers application fixtures and test code, excluding browser worker code. Executable arguments and environment values never enter the public report. `tests/mutation-runner-child-process.test.mjs` preserves these boundaries with real processes and Chromium.

`--runner auto|vitest|vite-plus|jest|playwright` selects the runner. Auto prefers root Vitest/Vite configs, then Jest configs or the package's `jest` field, then Playwright configs, then installed packages in that order. A project with Vite and Playwright configs must explicitly select Playwright. `--runner-config`, `--runner-project` and `--runner-package` configure the selected adapter. Existing `--vitest-*` flags retain their meaning. Jest CommonJS and Jest multi-project configs are unsupported. Browser tests use the [Playwright fixture](../playwright/README.md).

Real pinned fixtures live in `tests/fixtures/runners`. Install them with `npm ci --ignore-scripts --no-audit --no-fund --prefix tests/fixtures/runners`, install Chromium with that fixture's Playwright CLI, build the binary, and run `node --test --test-concurrency=1 tests/mutation-runners.test.mjs`. Check CI repeats these tests on Linux x64, macOS ARM64 and macOS Intel.

The native [generator](../../src/bin/errspec/mutation/source/mod.rs) uses the parser-only tsrs adapter. [Process ownership](../../src/bin/errspec/mutation/run.rs) is separate from the [pure judgment model](../../src/mutation/mod.rs). [Rust tests](../../tests/mutation.rs) snapshot syntax candidates and judgments; [emitter tests](../../tests/mutation-emitter.test.mjs) compare each active switch with applying its replacement directly. [Process tests](../../tests/mutation-process.test.mjs) check signals, deadlines, restarts, and cleanup with real workers. The [acceptance script](../../scripts/demo-mutation.mjs) checks statuses, identities, configuration, snapshots, concurrent coverage, and input changes with Vitest 5.

The real compiler acquisition lives in [`@errspec/typecheck`](../typecheck/README.md); the embedded driver imports it directly. It supplies diagnostics/equivalence facts, and native Rust maps those facts to mutation statuses. Public [`@errspec/vitest`](../vitest/README.md) and [`@errspec/jest`](../jest/README.md) entrypoints fix the runner while sharing the core request pipeline. `scripts/runner-package-smoke.mjs` proves offline tarball installation and real execution for these packages and the Playwright fixture.
