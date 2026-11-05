# Failure-atomicity design

## About this document

This implements the [Requirements](requirements.md) under the [Product](../../product.md)
using testkit dependency wrappers and state comparisons. It is intended for testkit
maintainers familiar with async tests and independent fixtures. Type scanning and
existing-test assertion acquisition remain separate features.

## Implementation

`checkFailureAtomicity` first runs a fixture without faults and records call counts
through named wrappers. For each fault/ordinal pair, create a fresh fixture and
run the same operation. At the selected call, invoke the user's fault function
instead of the real implementation.

For a failure value, compare `structuredClone` snapshots before and after the
operation with `deepStrictEqual`. Record successful outcomes separately as
`recovered`. Exceptions, unreached/incomplete faults, snapshot acquisition errors
and cleanup failures are violations. Execute all cases, then throw an error
carrying the report when violations exist.

## Flow and data

The sequence is fixture creation, operation, snapshot acquisition and cleanup;
there is no external process requiring a sequence diagram. The normal case maps
dependency names to call counts. Each fault case holds its ID, dependency, ordinal,
outcome, state and violation reason. These records are not persisted, so no ER
diagram is needed.

## API

`checkFailureAtomicity({ setup, faults, isFailure, only? })` returns a result Promise.
The fixture returned by `setup` supplies `run(scope)`, `snapshot()` and optional
`dispose()`. `scope.wrap(name, implementation)` wraps async dependencies while
preserving arguments and `this`. Each fault has a unique `id`, `dependency` and
`inject`; injection may return a value or throw.

`only: { fault, call }` replays one case. `FailureAtomicityError.report` contains
all case evidence; each case's `fault` holds its replay conditions. Arbitrary
JavaScript values mean the report is not guaranteed to be JSON-serializable.

## Integration and data handling

There is no extra external service or runtime dependency. Vitest, Bun and Node
tests can call the API. Clone snapshots before comparison to prevent mutations
through shared references from concealing changes. Call fixture cleanup in
`finally`. Keep counts and incomplete-call tracking per run.

Use the test's existing permissions. Trust the supplied fixture; explain that
snapshot diagnostics can expose its values so consumers should exclude secrets.
This is an additive testkit API and preserves existing scenario APIs; no special
deployment procedure is required.

## Alternative considered

Changing source alone can measure test sensitivity, but checking whether a normal
implementation leaves partial updates under realistic faults still requires
inputs and expected state. Explicit valid fault values and business state supply
that missing contract.

## Unresolved decisions

None for this feature's accepted scope.
