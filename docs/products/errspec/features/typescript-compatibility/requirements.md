# Select TypeScript 6/7 analysis backends

## About this document

Under the [Product](../../product.md), analyze with the consumer's TypeScript
configuration and type interpretation, reducing compatibility-driven config edits.

### Scope

Select tsrs or installed TypeScript 6/7 for type analysis in `scan`, `init`, `check`,
`verify`, `snapshot` and `coverage`. Include tag extraction, propagation facts and
pre-execution static matching. Record engine, version and selection reason.

### Outside scope

Mutant typechecking, new mutation rules, additional runners and automatic config
migration belong elsewhere. Preserve `mutate` parsing/execution contracts.

## User stories

### Preserve the consumer's configuration

**Scenario: use installed TS 6.** Given TS 6 accepts config containing `node10` or
`baseUrl`, automatic analysis uses that TS 6 without editing config and reports
tags and the selected engine.

**Scenario: use installed TS 7.** Given TS 7 and its required APIs are available,
automatic analysis uses its resolutions and reports version/selection reason.

**Scenario: retain tsrs when no TypeScript is installed.** Given no TypeScript in
the project/parents and no explicit package, automatic selection preserves the
existing tsrs analysis and result shape.

### Preserve judgments across backends

**Scenario: type errors stop before execution.** Given the selected TypeScript
reports errors, `check`/`verify` returns diagnostics and incomplete analysis, never
switching to another engine to turn failure into success. `verify` starts no tests.

**Scenario: shared propagation.** Given direct calls, recursion, catch, async and
dynamic calls, each backend supplies the same owned fact contract for common
judgment and explicitly reports dynamic/unresolved calls.

**Scenario: shared static mismatch rules.** Given included registration tests
mismatch contract values or required IDs, official-API `check` applies the same
`EC015`/`EC016` conditions as tsrs.

**Scenario: shared entry paths.** Given function declarations, const functions,
re-exports, async returns, throws and rejections, each backend's check or Bun
verification acquires the same return positions/tags for common `EC018`/`EC019`.
Nested functions' returns do not count as entry returns.

**Scenario: unavailable APIs are incomplete.** Given an unknown version, missing
API, startup failure or malformed response, report the cause instead of returning
empty tags or an empty scan as success.

**Scenario: honor explicit selection.** Given explicit tsrs or TypeScript, use it
and do not silently switch after failure.

## Compatibility measurement

Existing evidence covers TS 6.0.3/7.0.2 across the six semantic commands, comparison
with existing propagation/static matching, API failures and interruption, using
Node 22.21.1/24.8.0. The target is every scenario and same-input backend agreement.
Measure actual APIs/CLI results against existing expectations for this change and
each newly supported version.

## Functional requirements

**Input:** project, requested analysis and optional engine/package/Node overrides.

**Output:** results, diagnostics, engine, version, package and selection reason.

Automatic selection uses installed TS 6/7 APIs when available: a fallback triggered
only by failure cannot detect silent interpretation differences. Otherwise retain
tsrs. Required APIs are validated on TS 6.0.3/7.0.2; other versions need capability
checks and are not promised identical outcomes without comparison.

Do not alter tsconfig, source, tests or dependencies. Continue rejecting `noCheck`.
Compare tags, IDs, locations and ordering across backends. Do not download
TypeScript automatically. Diagnose setup for missing packages/Node and do not
claim that this semantic analysis typechecked mutations.

## Data and safety

Record provenance; compiler-independent tags/functions/origins/edges/unresolved
sites/registration facts; and original diagnostic codes, text and locations. One
request uses one backend, without mixing native compiler handles or type facts
from different engines. Source and paths remain local.

No external service is required. Execute explicitly selected packages/binaries as
consumer code, releasing analysis files and processes at completion. Expanding
version support requires API checks and comparison on that version; no other
scope decision is unresolved.
