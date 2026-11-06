# Journey practical trial: scope selection and test improvement

On 2026-11-05, isolated Journey use-case tests were used to check Git scope selection and follow up previously surviving mutants. The selected 29 mutants all survived the original tests. After adding 21 tests, the same 29 identities produced **23 Killed and 6 Survived**, with no implementation change. Confirmed new business bugs: **0**.

This source-free record retains aggregate observations and generic classifications. Private paths, exports, type/tag names, test bodies, source, raw reports, and trial scripts remain outside repository artifacts. The [earlier full/repeat comparison](https://github.com/jp-knj/errspec/blob/af04b10/docs/engineering/benchmarks/journey-2026-11-05.md) records the separate fixed-reference baseline; the [failure-verification trial](practical-failure-trial.md) covers other KARTE scopes.

## Conditions and preservation

The trial used the native debug binary from errspec `a481631`, macOS ARM64, Node 24.22.1, and the same Vitest 5.0.3 runner used in the fixed-reference comparison. Instrumented session reuse was enabled. Result/probe cache, weak mutation, arid filtering, and mutant typechecking were disabled. Consequently these observations do not measure cache reuse, weak filtering, or typecheck exclusions.

The original source and lockfile were unchanged. Trial scripts checked original source/lock content preservation; a final independent comparison found all 42 copied source files byte-identical to the originals and the original lockfile identical to its tracked revision. Additional tests and experimental Git state existed only in the isolated copy. No dependencies or original lockfiles were updated.

## Git scope selection

A temporary source edit was evaluated using Git scope selection against the collected inventory. The current baseline executed 146 tests. Of 818 candidates, **14 were selected and 804 omitted**; the selected results were **12 Killed and 2 Survived**. The run completed without Pending, NoCoverage, RuntimeError, or Timeout results.

Elapsed command time was **24.830 s** (24,829.5065 ms before rounding). Omitted candidates were outside that selected request, not newly verified or inferred kills. This demonstrates scope selection for one edit; it does not prove that every possible edit can safely use the same narrow selection.

## Targeted test improvement

The follow-up was a **nonrandom sample of 29 previously Survived mutants** from an inventory containing 78 survivors. Selection and manual test design focused on inspectable validation, malformed-input, node-category, and duplicate-handling cases. It was not a random experiment, and its outcome must not be extrapolated to the 78-survivor population.

The same selected keys were re-run before and after test additions. Every run completed, with no Pending, NoCoverage, RuntimeError, or Timeout results. The table reports elapsed command time, rounded to three decimal places.

| Stage | Baseline tests | Added tests versus original | Killed | Survived | Elapsed |
|---|---:|---:|---:|---:|---:|
| Original assertions | 146 | 0 | 0 | 29 | 36.393 s |
| First targeted additions | 165 | 19 | 18 | 11 | 49.321 s |
| Expanded targeted additions | 167 | 21 | 23 | 6 | 44.424 s |

Unrounded elapsed measurements were 36,393.369584 ms, 49,321.028833000004 ms, and 44,423.85825 ms respectively. These are individual runs, not medians or a controlled speed comparison. Extra test count does not map one-to-one to mutant count.

### Generic gap classifications

Conservative grouping of the 23 newly killed mutants yielded the following observed gaps:

| Classification | Mutants changed from Survived to Killed |
|---|---:|
| Collection-size upper bounds across five fields | 5 |
| Allowed enumeration values | 1 |
| Message-style node categories | 2 |
| Malformed data in message-style nodes | 5 |
| Popup-style node categories | 3 |
| Retaining the first duplicate | 2 |
| Malformed data in popup-style nodes | 5 |
| Total | 23 |

These are groupings of test sensitivity, not 23 independent bugs. Several edits can express the same missing assertion or behavior distinction. The implementation was left unchanged; added tests detected the selected altered implementations while the original continued to pass.

### Remaining six survivors

- Two boundary/optional-access edits were not distinguished by the ordinary dense-array cases under consideration.
- Two edits involved an undefined map key not queried through the string-ID lookup path exercised here.
- Two object-type-check edits followed an existing non-null check. Callable payloads and other values outside ordinary JSON could still distinguish behavior.

These observations explain why the selected follow-up did not kill those edits. They are not proofs of universal equivalence, and the six were not counted as new business defects or automatically approved as equivalent.

## Interpretation limits

The useful outcome was a concrete progression from observed survivors to additional assertions and changed verdicts. It does not establish the correctness of every expectation, all input/order combinations, or full Journey coverage. The 23 killed edits do not imply a detection-rate estimate for all 78 old survivors. This is also not evidence of superiority over mutator: the earlier comparison already found matching verdicts for common exact edits of these kinds.

Independent human investigation time was not measured. Recorded command durations cover execution only; broader agent wall-clock work included exploratory attempts and parallel activity. Neither supports a human-effort, review-efficiency, or performance KPI. One-stage timing differences must not be interpreted as speedups. Required CI/distribution acceptance remains separate in [implementation status](implementation-status.md).

## Durable public regression

[`tests/practical-mutation.test.mjs`](../../../tests/practical-mutation.test.mjs) and its [newly written fixture](../../../tests/fixtures/practical-mutation/) preserve representative boundary, duplicate, and malformed-input patterns without private project source. The real native test starts with three covered Survived edits, obtains three agent explanations, adds five tests, and verifies **the same three IDs become Killed** while implementation bytes remain unchanged. The recorded local native run passed in **5.650 s**.

With the documented Node 24/Vitest dependencies and a built native binary, re-run it from the repository root:

```sh
node --test tests/practical-mutation.test.mjs
```

`ERRSPEC_BIN` can select another built native binary; otherwise the test uses `target/debug/errspec`. [Check CI](../../../.github/workflows/ci.yml) includes this regression. It reproduces the public feedback pattern, not the private Journey experiment or its exact counts/timings. Private raw evidence and trial code are not required or shipped with it.
