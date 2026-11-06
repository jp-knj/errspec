# Bounded native source batching

This is an implementation/performance observation, not evidence that errspec is
faster or more accurate than mutator overall. The change reduces native parser
process starts without changing candidate generation, baseline execution or reuse
rules. The shared acquisition path serves Vitest, Vite+, Jest and Playwright.

## Fixed inputs and method

The before binary was built in release mode from errspec
`b6e8491d4d8a530732e0c04935bc0a1cbb54405c`; the after binary contains bounded
source batching on that base. The fixed reference is mutator 0.2.0,
`025870640d46e93e97285d67e4c265074cf8b295`. Measurements used macOS ARM64,
Node 24.22.2 and Vitest 5.0.5. Both binaries used the same installed dependencies.
[Raw timings](source-batching-2026-11-06.json) retain every observation.

Two experiments answer different questions:

- Source throughput generates candidates/metadata and validates emitted switches
  for 200 small TypeScript files with Unicode comments, five times. Individual and
  batched order alternates. Every complete generation/validation result must be
  deeply equal. This excludes test execution and is not a whole-command speedup.
- End-to-end mutation uses 12 source files and 12 tests of `n >= boundary`, with
  four mutant workers, weak mutation/arid/typechecking disabled. Errspec enables
  session/result/scope/probe reuse. Each of three repetitions resets the tool's
  cache, executes a cold run, then an unchanged warm run. Before/after order
  alternates. Reference snapshots and reports are outside acquired project inputs.

## Observations

| Measurement | Before | After | Interpretation |
|---|---:|---:|---|
| Source generation + emitted validation, median | 9.771 s | 1.177 s | 8.30× throughput for this source-heavy stage |
| Parser process starts for that input | 400 | 50 | Eight requests per child; complete results equal |
| Cold mutation command, median | 19.252 s | 19.105 s | No convincing whole-command improvement in three noisy observations |
| Warm mutation command, median | 3.641 s | 3.314 s | 8.96% shorter in this fixture; current ordinary baseline retained |

All 60 mutations were killed in every cold run. Every warm run reused 60 results,
executed zero mutation trials, and retained the current 12-test ordinary baseline.
The source-throughput comparison includes source digests, metadata, IDs, original
ranges, replacement text, ignored reasons and ordering, not only family counts.

The reference's cold/warm child-process wall-time medians were 12.130 s / 2.377 s;
its in-process `runMutation` medians were 9.902 s / 0.149 s. Errspec's numbers above
include process startup. The reference acquired 12 test files / 60 mutation trials
on cold runs and zero test files / zero trials on warm runs. These execution
contracts differ: errspec still executes the current ordinary baseline. Do not
compare errspec's process time directly with the reference's API-only time.

The exact-edit comparator found 60 common edits and 60 identical `Killed`
verdicts, with no unmatched edits: 12 BlockStatement, 24 ConditionalExpression and
24 EqualityOperator. This controlled all-killed boundary fixture cannot establish
general accuracy or superior defect detection. The reference API host exited 1
after cold mutant assertion failures, despite returning the complete 60-killed
report; every status is retained, and warm hosts exited 0.

The fixture is deliberately small. There are no browser, I/O, timeout or large-AST
performance observations here. Parser startup savings matter more when many files
are acquired, while runner lifecycle cost still dominates mutation trials and the
current ordinary baseline dominates this warm run. Broader practical workloads,
repeated timing distributions and final required platform checks remain necessary.

## Reproduction and regression evidence

Install the repository's pinned driver/checkout dependencies and build release
binaries for the base and current checkout. With the base binary retained outside
the checkout:

```sh
node scripts/benchmark-mutation-source.mjs \
  --before /path/to/base-errspec --files 200 --repeats 5
node scripts/benchmark-mutation.mjs \
  --before /path/to/base-errspec --output /tmp/errspec-observation \
  --reference /path/to/mutator-at-fixed-revision --repeats 3
```

The latter script requires an empty output directory, creates and removes its own
public fixture, saves raw reports/timings, compares before/after mutation facts,
and applies the existing exact-edit comparator to the optional reference. Install
the fixed reference's frozen dependencies first. A separate one-repeat smoke of
this committed harness passed; its timings were under concurrent verification
load and are excluded from the table.

[Emitter acceptance](../../../tests/mutation-emitter.test.mjs) additionally checks
mixed valid/invalid sources, byte-bounded batches, process starts, fingerprint and
malformed-response failures, aggregate-output overflow fallback, Unicode, inactive
semantics and active-switch/direct-edit equivalence. Existing baseline/cache,
scope, plugin, concurrency, ownership and runner suites remain the acceptance
gates; this benchmark does not replace them.
