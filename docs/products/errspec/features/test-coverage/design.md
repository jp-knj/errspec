# Existing-test coverage design

This implements the [Requirements](requirements.md) with a Vitest setup file and
temporary configuration, for maintainers of coverage acquisition and judgments.

## Implementation

1. Acquire type candidates as in `scan`. Merge re-exported duplicates of the same
   type/tags. Exclude `external` candidates as types to test in their dependency
   package, retaining their count. Keep types with empty `flows`: they may be
   errors consumed from dependencies or non-error unions; distinguish them in the
   display rather than excluding them.
2. Search upward from tsconfig's directory for the nearest `vitest.config.*`, or
   `vite.config.*`, and `node_modules/.bin/vitest`/`node_modules/vitest`, supporting
   workspace-root installations.
3. Write a temporary setup file and configuration that imports the consumer's
   unchanged config and appends the recorder to `setupFiles`. Resolve a relative
   root from the original config's directory.
4. Run `vitest run --config <temporary-config> --reporter=json` in the configuration
   directory, forwarding arguments after `--` unchanged.
5. Wrap value-comparison matchers on the test's own Vitest chai. Only after the
   original matcher succeeds, and only for a non-negated assertion, record string
   discriminator values required by its expectation. Omitted partial-match
   properties, asymmetric matchers and object reference equality are not evidence.
   Direct string comparisons and string array-element comparisons remain recorded.
6. Pure judgments classify a discriminator/tag pair or matching string record as
   `asserted`. Otherwise, a quoted tag in the body of a runner-reported test file
   is `mentioned`; the remainder is `missing`.
7. Deduplicate totals by discriminator property and tag value. Several types with
   one pair count once; equal values with different properties count separately.

## Exit states

Keep runner success separate from record acquisition/validation/aggregation.
Unreadable or invalid records produce `EC_RUNNER` and exit 2; an empty aggregate
cannot stand in for successful verification. Retain actual execution and runner
status even when evidence is invalid.

After completed aggregation, runner failure, missing tags under `--fail-on-missing`,
a baseline regression or new missing tag produces exit 1. Otherwise exit 0.

## Baselines

Do not save or compare a baseline when observations cannot be validated.
`--save-baseline` stores per-type (`file#export#discriminant`) tag states without
overwriting existing files. `--baseline` orders strength as `asserted`, `mentioned`,
`missing`. Weaker tags are `regressed`, new missing tags are `newMissing`, and both
fail. Stronger tags are `improved` and informational. Removing a type/tag is not a
regression: it left the coverage inventory rather than losing a test assertion.

## Reasoning

Requiring `scenarioTest` would prevent existing assertions from supplying evidence.
A string search or traversal of the whole received object would incorrectly count
mock values or properties omitted by partial expectations. Prioritize values
required by successful comparisons. Retain `mentioned` because a test that injects
a dependency fault and asserts only its result can still check failure behavior;
users need to see the difference in evidence strength.

## Limits

Vitest is supported; Bun's expect is not chai and needs different acquisition.
Match records to file and full test name and accept only uniquely matched successful
tests. A comparison in a test that ultimately fails is not evidence. Do not prove
which error type originated a shared tag or whether an expectation is correct.
Leave asymmetric matchers unresolved. Never rerun the function passed to `toThrow`;
record Error properties received by `rejects`.
