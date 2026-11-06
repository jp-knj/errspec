# Discover error candidates and propagation paths

## About this document

Under the [Product](../../product.md), investigate failure sources and their
propagation across a project before registering contracts.

### Scope

For typed TypeScript sources, discover finite discriminator types, explicit
throws, `Promise.reject`, and returned values with a structural `error` property.
Inspect functions, methods, arrows, cross-module calls, recursion, await, return
and catch, relating origins to callers.

### Outside scope

Do not guarantee exhaustive runtime errors in arbitrary JavaScript. State unknown
behavior for dynamic dispatch, external implementations, callback execution rules,
getters/proxies, eval, implicit exceptions and whole-path feasibility. Users decide
whether a finite tagged type represents a business error.

## User story: choose types and failure sources from evidence

**Scenario: exported finite tagged types become usable candidates.**

- Given a project with imports/re-exports and a finite tagged type.
- When scanned.
- Then report file, public name, discriminator and tags. Do not present an arbitrary
  string as a finite inventory.
- Merge aliases/re-exports of one declaration into one candidate, preferring the
  declaring file's public name and listing other export paths.
- Mark, without excluding, a type whose discriminator declarations all belong to
  dependencies or files outside the project as external.
- Show Result/throw/rejection channels when the type occurs at a failure source.
  Keep candidates that do not occur at such a source.

**Scenario: a failure traverses several functions.** Given a throw in another
module reached through aliased imports or recursion, scanning shows its origin
and call path and terminates even with cycles.

**Scenario: awaited failures and directly returned promises differ.** Given one
function catches an awaited failure and another directly returns a Promise,
scanning distinguishes the catch boundary from rejection propagation. Show
rethrows and transformed errors as sources too.

**Scenario: unknown callees are not reported as failure-free.** Given an external
implementation, function parameter sharing a known function's type, mutable alias
or `any` prevents resolving the implementation, scanning reports its position and
reason. Strict unknown checking exits nonzero.

## Measurement

Measure candidates, unresolved sites and scan duration on real projects. Baseline
values and speed targets are unmeasured. Do not calculate an exhaustive failure-
discovery rate without a known denominator.

## Functional requirements

**Input:** tsconfig and candidate discriminator properties.

**Output:** structured or text candidates, origins, functions, call relations,
propagated failures, unresolved sites and analysis assumptions.

Propagation is a conservative possibility analysis. It does not prove catch
conditions or return-value meaning. Do not change source/configuration or run
tests. Type errors and analysis failures produce diagnostics and nonzero exits;
analysis completion is separate from knowing every runtime error.

## Data and relationships

Candidates carry file, public name, tags, other exports of their declaration,
external status and channels in which they appear as errors. Origins/functions
carry positions, types and failure channels. Unresolved sites carry position,
caller and reason. Several functions can reference one origin through call edges.
Names and source paths appear in output but are not automatically sent elsewhere.

## Dependencies and safety

No external service is required. Analyze local source and dependency type
information without executing target code.

## Future decisions

Add library-specific Result/callback semantics separately when real projects need
them, specifying both recognized and unresolved conditions.
