# Error-tag coverage from existing tests

## About this document

Following the [Product](../../product.md), report which error tags existing Vitest
tests assert without requiring errspec-specific test APIs.

### Scope

For finite tagged types, run the project's Vitest and aggregate tags required by
successful tests' assertion expectations. A tag appearing only in the received
value is not asserted. Distinguish quoted tags mentioned in a test file from tags
absent from all test files.

### Outside scope

`scenarioTest` and `verify` retain the stronger fault-completion and required-
comparison guarantees. Bun, semantic classification as a business error, and
automatic test generation are outside this feature.

## User stories

### Find unverified errors without rewriting tests

**Scenario: a successful assertion requires an exact tag.** Given a passing test
with `expect(result.error).toEqual({ type: "NOT_FOUND" })`, running
`errspec coverage` marks `NOT_FOUND` as `asserted` and names the test.

**Scenario: received tags alone are insufficient.** Given only
`toMatchObject({ ok: false })` or a `toHaveProperty` that does not specify the tag,
coverage does not count a tag in the received result as `asserted`.

**Scenario: a later test failure invalidates an earlier successful comparison.**
Given a matching tag assertion followed by a test failure, coverage does not use
that comparison as asserted evidence and also reports the runner failure.

**Scenario: a tag is mentioned but not compared as a value.** Given a test writes
a tag only in a mock return and compares the HTTP status, `errspec coverage`
classifies it as `mentioned`, not `asserted`.

**Scenario: no test mentions a tag.** Given an implementation tag absent from every
test, `errspec coverage --fail-on-missing` reports `missing` and exits 1.

**Scenario: equal values with different discriminators remain distinct.** Given
`code: "Shared"` and `kind: "Shared"` types and a test asserting only the former
without spelling the tag directly in its body, coverage reports the former as
`asserted` and the latter as `missing`; `--fail-on-missing` exits 1.

**Scenario: invalid observation data cannot produce a successful aggregate.**
Given tests pass but the record file contains invalid JSON or cannot be read,
coverage reports `EC_RUNNER` and exits 2. Actual execution and runner success
remain reported separately from the invalid record.

**Scenario: negative and failed assertions are not evidence.** Given a failed
assertion or `.not` receives a tagged value, do not count it as `asserted`.

### Prevent regression while retaining known gaps

**Scenario: a saved state becomes weaker.** Given a baseline saved with
`errspec coverage --save-baseline coverage-baseline.json`, compare with
`--baseline coverage-baseline.json`. An `asserted` tag becoming `mentioned` or
`missing`, or `mentioned` becoming `missing`, is a reported regression and exit 1.

**Scenario: a new tag is unverified.** Given a newly added tag absent from all
tests, baseline comparison reports a new `missing` tag and exits 1.

**Scenario: unchanged gaps and improvements pass.** Given a retained gap or a
stronger state, baseline comparison reports improvements and exits 0 when there
is no regression. It does not overwrite the existing baseline.

## Functional requirements

**Input:** tsconfig, discriminator properties, discovered or explicit Vitest
executable/configuration, and extra `vitest run` arguments.

**Output:** per-type tags classified `asserted`, `mentioned` or `missing`, asserting
tests, aggregates and runner status. Do not alter assertion outcomes, test results,
consumer configuration or test files.
