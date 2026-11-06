---
name: errspec-test-loop
description: Close errspec mutation gaps in a branch or report by adding behavior assertions, rechecking selected mutants, and distinguishing unresolved or equivalent cases from detected changes.
license: MIT
---

# Close mutation gaps

Use an installed or verified artifact `errspec`; do not assume an unpublished npm
package is downloadable. This workflow assumes the project's ordinary baseline
passes. For installation or runner setup, use the shipped `errspec` skill.

```sh
mkdir -p .errspec
errspec mutate --since '<base>' --cache --reporter agent > .errspec/agent.json
```

Read `warnings`, `runComplete`, and `summary` before selecting a gap. Exit 1 is a
quality failure whose report can still be complete. Exit 2 requires fixing the
configuration, baseline or incomplete execution first. A budget may leave Pending
candidates and an unknown score; preserve that limitation in the result.

Group `gaps` by `scope.id`, inspect `scope.source`, and read the actual test files
behind `coveredBy`. Keep opaque test/mutant IDs unchanged. `coveredByCount` may
exceed the bounded list shown in the agent report.

- `notReached`: add a case that reaches the branch or fallback. A child-process
  warning means the worker may not observe execution that happens elsewhere.
- `notInfected`: choose an input where original and replacement differ, such as
  the boundary for `>=` becoming `>`.
- `notDetected`: assert the changed return value, error, state or externally
  meaningful call. Check separating inputs before adding redundant assertions.

Follow existing test style and helpers. Verify behavior a caller can observe;
avoid pinning private implementation solely to kill a mutant. If a test reveals a
real defect, describe it and fix it within the requested scope. Tests must clean
up only resources they created, never arbitrary PIDs/paths returned by code that
is currently mutated.

```sh
errspec mutate --only '<key>' --cache --reporter agent
```

A disappeared gap is insufficient by itself: confirm the selected ID still
exists, ran, and is detected or has a justified exclusion. Unknown IDs require a
fresh report. Selected-run scores and gates apply only to that selected inventory.

Accept an equivalent case only when the reasoning is established for the relevant
inputs and recording acceptance fits the requested work:

```sh
errspec accept '<key>' --reason 'equivalent: specific observable reason'
```

Do not invent equivalence, weaken assertions, or accept uncertain cases just to
make a gate pass. Keep unresolved cases and their reasons visible. Review stale
or newly detected acceptance entries reported by later runs.

Finish by rerunning the relevant diff/full inventory with the new tests. Report
which behavior checks changed, detected gaps, reasoned acceptances, unresolved
cases, Pending/incomplete work, and any actual defect. Do not claim that a subset,
restored cache, or passing mutation run proves complete business correctness.
