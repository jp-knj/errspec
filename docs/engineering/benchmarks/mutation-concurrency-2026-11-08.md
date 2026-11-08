# Mutation concurrency and coordinator extraction

This experiment compares one, two and four mutation workers. It does not establish
a general speedup or a before/after performance change from extracting functions.
The implementation keeps the existing scheduling, verdict and persistence rules.

## Fixed inputs and method

The before binary is a release build of `56a496647ff73232a70fbc1229a585a2c120859c`
(proposal B merged). The after binary contains the coordinator phase extraction
on that base. Both use Rust 1.99.1, Node 24.8.0, Vitest 5.0.5 and the same installed
dependencies. The host is macOS ARM64 on an Apple M2, with eight logical CPUs and
16 GiB RAM. [Raw observations](mutation-concurrency-2026-11-08.json) include binary
SHA-256 values, fixture hashes, every retained timing and memory peak.

The public synthetic fixture matches the existing mutation benchmark: 12 source
files containing `n >= boundary`, 12 boundary tests and 60 candidates. Each run
uses fresh runner sessions, no result/probe cache, and default mutation selection;
weak mutation, typechecking and sampling are disabled. The complete CLI command,
including baseline/probe, worker startup, trials and shutdown, is timed.

Each binary runs three repetitions in the orders `1,2,4`, `2,4,1`, `4,1,2`.
The before block precedes the after block; the binaries are not interleaved.
The harness samples `ps` PID/parent-PID/RSS data every 100 ms and sums RSS for the
CLI and its current descendants, including supervisors, Node and runner workers.
Reported memory is the median of the three sampled aggregate-RSS peaks. It is
not private memory, unique physical memory or a continuously measured maximum:
shared pages can be counted repeatedly and brief peaks can be missed. The same
sampler runs for every worker count and contributes overhead to elapsed time.

## Observations

| Workers | Before median (range), seconds | Before peak RSS, MiB | After median (range), seconds | After peak RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| 1 | 7.481 (7.410–8.262) | 293.2 | 10.448 (8.353–30.205) | 304.7 |
| 2 | 5.582 (5.508–5.644) | 508.9 | 6.710 (6.614–14.331) | 517.3 |
| 4 | 5.300 (5.030–5.719) | 888.9 | 8.656 (6.898–19.140) | 902.7 |

The before block reduced median wall time by about 25% from one to two workers;
two to four saved another 5% while increasing summed peak RSS by about 75%.
This small fixture does not justify increasing the default or adding concurrent
startup/file hashing. Two workers are a useful starting point for this fixture,
not an automatically selected setting or a recommendation for every application.

The after block encountered substantial system contention despite running without
other verification launched by this task. One mid-block observation recorded a
one-minute load average of 21.5 on eight logical CPUs and about 3.8 GiB swap in
use. The wide ranges prevent attributing the before/after time difference to the
refactor. An earlier after block overlapped a local build and was excluded from
this table; its raw observations are retained with that reason. No further samples
were removed from the reported blocks. A quiet-host, interleaved comparison and
larger practical workloads are needed before making performance claims.

Every reported run completed all 60 mutations with `Killed` verdicts and retained
the current 12-test baseline. The harness compares the complete ordered mutant
records, including edits, positions, covering/killing tests and attempt evidence,
plus completion, summary, gates, diagnostics and baseline identities/states.
Timing and internal task IDs are omitted from that comparison. The before/after
semantic JSON hashes match. This all-killed fixture is complemented by the
existing survivor, timeout, cancellation, cache and partial-result tests.

## Implementation decision

Keep concurrency opt-in with default one and the existing independent worker
pool. Split the coordinator into configuration/session setup, preparation,
planning, execution and finalization, as described in the
[execution guide](../mutation-flow.md#coordinator-phases-and-ownership).
`run.rs` decreases from 1,483 to 148 lines; `run_inner` contains six phase calls.
Partial reports, per-candidate sequential ordering, the common time budget and
driver-before-temporary-resource destruction remain explicit. The public API and
snapshot expectations do not change. Source changes invalidate old engine-bound
caches through the existing fingerprint algorithm.

## Reproduction

Build and retain the base release binary, then build the current release binary.
Install the repository's locked checkout/driver dependencies. Use a new output
directory; the second command reuses the first command's exact fixture and checks
its hashes. The script requires permission to inspect process RSS with `ps`.

```sh
node scripts/benchmark-mutation-concurrency.mjs \
  --binary /path/to/base/errspec --output /tmp/concurrency-observation
node scripts/benchmark-mutation-concurrency.mjs \
  --binary target/release/errspec --output /tmp/concurrency-observation --label after
```

The output retains fixture inputs, complete native reports, per-run RSS samples,
metadata, semantic comparisons and timings. It never replaces an existing label's
observations. Keep compilation and tests separate from timed runs. The benchmark
does not run in CI and does not introduce a runtime dependency or performance gate.
