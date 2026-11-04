# Embedded mutation driver

The Rust binary embeds `dist/driver.mjs` and `setup.mjs`. End users need Node 24+ and their own Vitest 5.x installation. They do not build or install this private package.

`driver.mjs` owns Vitest sessions and the versioned NDJSON protocol. `emitter.mjs` applies the native generator's byte ranges with MagicString. `setup.mjs` records worker hits and test metadata before user setup files execute. The Rust CLI owns judgments, deadlines, temporary files, and the process group.

After editing driver or emitter source, rebuild the committed bundle and notices with Bun 1.4.4:

```sh
npm ci --ignore-scripts --no-audit --no-fund --prefix packages/mutation-driver
node scripts/build-mutation-driver.mjs
node scripts/build-mutation-driver.mjs --check
cargo build --locked
node --test tests/mutation-emitter.test.mjs
node scripts/demo-mutation.mjs
```

Vitest is imported from the selected installation and is never bundled. No mutation cache persists between CLI invocations. The temporary state file only supports restarting the driver after a deadline.

The native [generator](../../src/mutation_source.rs) uses the parser-only tsrs adapter. [Process ownership](../../src/mutate.rs) is separate from the [pure judgment model](../../src/mutation.rs). [Rust tests](../../tests/mutation.rs) snapshot syntax candidates and judgments; [emitter tests](../../tests/mutation-emitter.test.mjs) compare each active switch with applying its replacement directly. [Process tests](../../tests/mutation-process.test.mjs) check signals, deadlines, restarts, and cleanup with real workers. The [acceptance script](../../scripts/demo-mutation.mjs) checks statuses, identities, configuration, snapshots, concurrent coverage, and input changes with Vitest 5.
