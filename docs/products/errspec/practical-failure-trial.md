# KARTE practical trial: existing tests and fault verification

On 2026-11-05, isolated copies of Obel VCS/AST VCS and Agent Studio backend were used to exercise real implementation and existing Vitest tests. Original source, lockfiles, and dependencies were unchanged. Experimental configuration, scenarios, and detailed reports stayed in temporary directories. This document retains aggregates only: no private code, type names, tag values, test bodies, or absolute paths.

## Conditions and scope

The native errspec implementation was `a642c2b488343c1b26603433294a07305b734f5e`. The environment was macOS ARM64, Node 24.22.1, TypeScript 5.9.3, Obel Vitest 4.1.3, and Agent Studio Vitest 3.2.4. TS 5 is outside the official semantic API backend's selection, so the native backend analyzed it. No new dependencies were fetched and existing lockfiles were not updated.

VCS used all existing tests. AST VCS excluded one performance-test file. Agent Studio selected two existing authentication/error-classification files that require no external service; this is not a backend-wide coverage measurement. Before execution, every copied `src` file's relative path and SHA-256 matched its original. Experimental edits were restored after each trial, and the original repository remained unchanged.

Experimental tsconfigs lived inside the copies and extended existing settings. They explicitly resolved Node declarations/read-only dependencies and placed Vite caches in temporary storage. An earlier attempt put tsconfig outside the copy and included original files by absolute path. Those files were outside the config-directory scan boundary, producing zero candidates despite successful scanning. That result was excluded from coverage evidence. Adoption must inspect candidate counts and selected files rather than inferring correct scope from successful exit alone.

## Tag comparisons in existing tests

Each target was run once in sequence. These are observations, not performance guarantees or statistical comparisons. Direct runner duration is separate from `coverage`, which also acquires types and assertion evidence.

| Target | Existing files / tests | Pass / fail / skip | Direct execution | Coverage command | Local candidate types | Tags | Asserted | Mentioned | Missing |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| VCS | 14 / 151 | 151 / 0 / 0 | 4.119 s | 4.826 s | 30 | 29 | 25 | 0 | 4 |
| AST VCS | 43 / 908 | 908 / 0 / 0 | 19.901 s | 22.379 s | 32 | 47 | 42 | 4 | 1 |
| Selected Agent Studio scope | 2 / 111 | 111 / 0 / 0 | 1.988 s | 2.404 s | 2 | 14 | 12 | 0 | 2 |

All targets had `runnerOk: true` and zero analysis/execution diagnostics. Of 37 AST VCS scan candidates, five declared in dependencies were excluded from default coverage. Tag counts deduplicate discriminator-property/value pairs rather than summing repeated type rows.

`asserted` proves a tag-value comparison in a currently passing test. It does not prove execution of every function/path returning that type and can match separate declarations sharing the same value. Generalized acceptance tests retain this limit.

## Useful findings and noise

- VCS's four missing values existed in declarations without construction sites in that package's implementation. They inform tests/public-contract review rather than establish implementation defects.
- AST VCS's four mentioned values appeared in a type-exhaustiveness switch without executed comparison evidence. Distinguishing source text from execution was useful. Its one missing value also lacked existing test evidence and requires specification-informed investigation.
- Agent Studio's two missing values belonged to declared error families without generation evidence in the selected scope. They do not establish missing tests across the whole backend.
- Structural discovery also includes ordinary data types. Five of AST VCS's 32 local candidates, containing 11 deduplicated values, were ordinary resource categories and noise for failure investigation. VCS had one mixed result type with one success value. Manual failure-only reference counts were 24 asserted of 28 VCS values and 31 asserted of 36 AST VCS values. This manual classification is not an automatic semantic judgment by errspec.

Confirmed new business bugs: **0**. Seven missing and four mentioned values were useful investigation candidates, not generated bug reports. Reexports and type inclusion can repeat values across rows; review deduplicated values together with their declarations.

## Fault injection and negative controls

Scenarios called real implementation and injected intended failures at the VCS storage port, the AST VCS initial-read port, and Agent Studio's external-client boundary through a module mock. Agent Studio verified propagation of declared typed failures, not every authentication branch. AST VCS and Agent Studio also required assertions that subsequent operations were not called.

| Target | Registered cases | With fault / explicit no-fault | Normal verification | Duration | Seeded failure-handling regression |
|---|---:|---:|---|---:|---|
| VCS | 4 | 3 / 1 | All tests and required observations pass | 1.246 s | Three fault cases fail; no-fault case still passes |
| AST VCS | 3 | 3 / 0 | All tests and required observations pass | 2.573 s | All three fail |
| Agent Studio | 10 | 10 / 0 | All tests and required observations pass | 1.652 s | All ten fail |

Removing a mandatory result assertion produced EC010 and bypassing the fault wrapper produced EC009 for each target. For AST VCS's three cases and Agent Studio's ten, another assertion remained, so Vitest itself passed while errspec detected the missing required assertion. Removing VCS's sole assertion also caused testkit to fail the test. Removing all fault wrappers was rejected as incomplete by testkit; VCS's explicitly fault-free case remained successful.

Seeded implementation changes converted failure to success or replaced the original typed failure with another failure. Verification rejected all three target-level changes. These were **three intentionally seeded regressions**, excluded from discovered bug counts. Seventeen passing scenarios establish selected behavior, not all inputs/orderings or automatic failure-atomicity verification.

## Adoption and investigation time

Elapsed time from first isolated configuration to each target's final selected coverage was 3 min 26 s, 4 min 20 s, and 6 min 56 s. All three initial fault-verification results were available after 7 min 32 s; all negative controls after 15 min 35 s. These are agent wall-clock observations including setup, dependency resolution, reading existing tests, and execution, with some parallel work. They do not measure human labor, review efficiency, or separate active investigation costs.

Required adjustments were the scan boundary, Node-global type resolution, monorepo parent dependency resolution, and choosing the intended test scope from existing Vitest configuration. Original systems required no changes. Candidate counts and executed files must be checked explicitly.

## Durable public acceptance

[`scripts/demo-failure-boundaries.mjs`](../../../scripts/demo-failure-boundaries.mjs) creates a newly written, nonprivate read/write fixture in temporary storage. Actual Vitest tests exercise existing tag comparisons, failure propagation, blocked later writes, a missing mandatory comparison in a passing test, wrong-ID fault observations, failure converted to success, and a write after failure. Check CI includes it.

Re-running this public acceptance is distinct from reproducing the private-system measurements. Detailed private reports and trial code were not copied into public artifacts. Overall completion and CI limitations remain in [implementation status](implementation-status.md).
