# Failure scenario registration and execution verification

This feature implements the correspondence between declared error variants and failure scenarios in [Product](../../product.md). Its guarantees are limited to the registrations and evidence described here.

## Scope

For explicitly selected TypeScript error types, check variant registrations, scenario execution with Bun/Vitest, required fault observations and assertions, and changes from a saved scenario inventory. Multiple scenarios may represent one variant.

[Discovery](../discovery/requirements.md) separately handles error candidates and propagation. This feature does not enumerate every input, order, or timing combination; judge whether expectations are correct; execute natural-language specifications; or resist malicious test code. Mutation testing is specified separately.

## User stories and acceptance

### Detect missing registrations

Developers need to see when an error-type change introduces an unregistered failure variant.

- Adding a variant without a case reports that variant and fails registration checking.
- Valid registrations alone can pass `check`, but tests and observations remain unexecuted.
- An unconstrained discriminator that cannot yield a finite tag set reports incomplete analysis rather than success.

### Verify required observations and assertions

Developers need current execution evidence even when the runner itself reports success.

- Separate first-attempt and post-retry scenarios for the same tag each pass only when their uniquely matched current test, required completed fault calls, and mandatory assertions pass.
- An input-only scenario may pass without fault observations only when configuration explicitly declares an empty fault list and the test explicitly declares that it uses no faults. Mandatory assertions remain required.
- A no-fault declaration on only one side fails: configuration-only causes test failure; test-only leaves required faults unobserved.
- Removing a mandatory assertion fails observation verification even if the test passes.
- Preparing a fault function without calling it does not count as an observed fault.
- Skipped tests do not establish verified scenarios.
- Evidence from another run cannot establish current success.
- A scenario inside one or more `describe` blocks matches despite suite prefixes. Registering the same scenario in multiple suites is ambiguous and fails.
- An optional human-readable description after the stable test identity does not change scenario identity or baseline comparison. Having both described and undescribed versions in the same file is ambiguous and fails.

### Detect scenario removal and change

Developers compare with a saved inventory to distinguish an intentional specification change from lost verification.

- Removing one of two scenarios for a tag fails even when the tag remains covered by the other scenario.
- Changing expected behavior or required observations appears in the diff. The default comparison does not fail solely for this change.
- Strict comparison fails a changed scenario, including deletion of a mandatory assertion from both configuration and test, even when the test still passes.
- Adding scenarios while preserving existing ones does not itself fail strict comparison.
- Strict comparison without a baseline, with an old format, or with missing scenario history fails with an error instead of reporting no change.

Strict comparison applies to observation-enabled registration checking and verification. Combining it with inventory generation is rejected before creating a file. It compares expected behavior, tag, test reference, and mandatory observations, including observation-ID order under the existing comparison rules. It does not determine whether a change semantically weakens verification.

An intentional change requires diff review and an updated inventory. The tool does not record or prove human approval. CI supplies a comparison baseline separate from the proposed change.

## Functional requirements

Inputs are the exported error type, cases, test references, required faults/assertions, and optional baseline. Outputs include verdicts and reasons in text and structured form, with test execution status separate from observation status.

The selected public type must expose finite string discriminator values. Verification requires exactly one current successful test per scenario, all required observations/assertions, and a successful overall runner execution. A fault observation establishes that a developer-selected function was called and completed. It does not establish that its value represents a meaningful business fault or that an assertion is sufficient.

Legacy registrations retain their existing test matching and are visibly distinguished from the newer observation-enabled format.

Invalid configuration, type errors, runner startup failures, corrupt records, ambiguous duplicates, and unfinished observations cannot pass. A known runner version that omits test-file identities is rejected before tests start. Partial test success does not override overall runner failure. Old execution results are not reused to pass current scenario verification. Inventory creation never overwrites an existing file.

For Bun 1.2.18 or earlier, which omits JUnit suite file attributes, report the detected and required version and stop before execution.

### Static checks before execution

When registered test files belong to the analyzed project, check their declarations against configuration before running tests. Report only mismatches that necessarily fail at runtime; leave unresolved values to runtime matching.

- A scenario ID resolved to concrete values, including an unregistered value, reports the scenario and source location and fails checking.
- Concrete fault/assertion IDs directly used in the scenario body must belong to the configured required IDs.
- An arbitrary string or helper API used outside the directly inspected scenario body does not fail static checking merely because its value cannot be established.

### Explicit selection of registered test files

`verify --only-contract-tests` runs only files referenced by registered cases. The default continues to execute all tests. An empty selection is a configuration error; it must not silently expand to all tests. Unselected tests are not reported as passed.

When registered tests pass and unrelated tests fail, explicit selection judges the selected execution and evidence. Default full execution still fails for the unrelated runner failure.

## Data and trust

Type identity locates the type and variants. Case registration supplies scenario identity, expected behavior, test reference, and required observations. Execution supplies test state, fault call/completion counts, and assertion results. The saved inventory supplies historical tags and scenarios. One type has multiple variants, each variant may have multiple scenarios, and each scenario requires unique current test/evidence matching.

There is no required external service. User tests may connect to services or incur their own costs. Identifiers, paths, descriptions, and logs can contain personal data; users review them before sharing.

Tests and the observation helper are trusted executable code. The tool prevents accidental stale or unregistered evidence from establishing success; it is not a hostile-code sandbox.

## Evidence and deferred decisions

Developer time savings and reductions in missed verification have not been measured. Product KPIs require a real adoption trial; test counts are not product-impact measurements. Partial acceptance of individual scenario changes and semantic classification of changes remain outside this scope. Default comparison behavior remains compatible.
