# errspec

## Overview

errspec connects TypeScript error variants with explicit failure scenarios and
test evidence. It helps developers and AI-assisted changes reveal a new error
variant, an omitted failure case or a check that never executed. In addition to
checking errors asserted by existing tests, it injects developer-specified faults
into operations to reproduce partial business-state updates on failure.

[Failure-verification requirements](features/failure-verification/requirements.md)
define the initial contract. [Discovery](features/discovery/requirements.md) helps
find candidate registrations, and [distribution and initial generation](features/distribution/requirements.md)
helps consumers start from an existing type. Analysis preserves the project's
TypeScript configuration and type interpretation through the
[TypeScript backend contract](features/typescript-compatibility/requirements.md).

The product target includes every capability of
[mutator 0.2.0 at the fixed comparison revision](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/README.md),
alongside errspec's error-type and failure-scenario verification. The
[capability matrix](mutator-coverage.md) distinguishes that target from implementation
and validation evidence. [Base mutation testing](features/mutation-testing/requirements.md)
provides generation, execution and verdicts through `mutate`;
[additional workflows](features/mutation-workflows/requirements.md) define repeated
use, selection, caching, adapters, reports and integration.

The full implementation plan and feature scope are authorized. That approval is
not evidence that every acceptance check, required CI platform or practical trial
has completed. [Implementation status](implementation-status.md) records them.

## Problem

Adding a TypeScript error variant does not establish that tests cover its failure
behavior. Passing tests may never produce the required fault or may omit the
comparison that would detect an incorrect result. Developers need evidence that
connects declared errors, failure cases, actual execution and observations.

The starting point was the discussion of extending error handling from expressing
errors to checking behavior on failure. The tool reports omissions and unresolved
analysis without claiming to discover every runtime failure.

## Users

Developers maintaining TypeScript/JavaScript tests, including those delegating
implementation and test changes to AI. People determine expected business behavior;
the tool supplies traceable evidence during local development and PR verification.

## Value and success

When failure-handling code or error types change, users can identify missing
verification, understand its cause and choose the next test or fix. Test count and
pass rate alone are not success measures. Mutation results should help explain
why a change escaped detection; restoring a cache or executing code alone does
not establish correctness.

## Development principles

- Never treat an unverified or incomplete result as success; state its scope.
- Use the consumer's existing error representation and test environment.
- Make results usable by people and agents without an editor integration.
- Keep the expected behavior and adequacy judgment with the user. Tool success
  does not prove the correctness of the application's business specification.
- Distinguish a requirement change from an implementation improvement and explain
  the reason for each change.

## Guarantees and reliability measures

There is no availability or response-time SLA for this locally executed development
tool. Numeric performance SLOs are not defined. Compare representative consumer
projects before making speed or effectiveness claims; adopting Rust alone is not
performance evidence. Acceptance conditions require that missing evidence cannot
create a false success.

## Boundaries

A scan reports possibilities and unresolved areas; it does not prove exhaustive
runtime-error coverage. Mutation inclusion preserves existing errspec interfaces
and compares capabilities rather than identical CLI/configuration/JSON/plugin APIs.
Repository migration, visibility changes, npm/crates publication and production
deployment remain separate future work.
