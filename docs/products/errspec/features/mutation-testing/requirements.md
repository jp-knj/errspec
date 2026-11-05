# Mutation generation, execution, and judgment with Vitest

[Product](../../product.md) requires checking whether existing tests detect code changes. This document defines the initial `mutate` capability: 18 built-in mutator families, baseline and per-mutant execution, text/native JSON, and an optional gate for undetected mutants. The broader approved scope is in the [workflow requirements](../mutation-workflows/requirements.md) and [capability matrix](../../mutator-coverage.md).

## Scope and boundaries

The initial adapter targets one selected Vitest 5 Node project and JS/TS modules transformed through Vite. It does not require exported error types, `errspec.json`, or rewriting tests to `scenarioTest`. It includes source/family selection, per-test reach, module-initialization reach, and explicit execution evidence.

Git selection, persistent cache, weak mutation, compiler typechecking, arid/comment/plugin exclusions, acceptance, parallelism/budgets, Jest/Playwright/Vite+, richer reports, matrices, and thresholds belong to the separate workflow specification. They are not missing from the product merely because this foundational document isolates their design. Bun, Vitest Browser Mode, Vue/Svelte single-file components, native require outside the transform path, and propagation into arbitrary subprocesses are not guaranteed by this Vitest adapter.

[Failure verification](../failure-verification/requirements.md) and [tag coverage](../test-coverage/requirements.md) remain independent. Mutation verdicts do not establish mandatory fault observations, assertion sufficiency, or correct business expectations.

## User stories and acceptance

### Find changes existing tests miss

- Each of the 18 families produces candidates for inputs where a safe inactive switch is possible; an entire safely applicable family cannot be omitted for implementation convenience.
- Given passing boundary tests for `age >= 18`, changing `>=` to `>` is `Killed` when the test with 18 fails. Report original location, edit, and failing test.
- If only 19 is tested and all selected tests pass, the reached edit is `Survived`. This says nothing about whether the original business rule is correct.
- A candidate with no baseline reach is `NoCoverage`, with its skipped trial visible. It is not a successful mutant execution.
- Runtime strings can be mutated while type literals/import specifiers remain intact. Unsafe placements have explicit exclusion reasons.

### Establish a valid baseline

- Failure of an ordinary unmodified test run prevents mutant trials, produces an incomplete result/nonzero exit, and does not count as a kill.
- No runnable tests, including an all-skip/todo suite, produces no score and a nonzero exit.
- An inactive instrumented probe must preserve the ordinary baseline's test inventory and outcomes. A mismatch prevents trials.

### Explain failures and incompletion

- A reproducible mutant-induced module initialization failure is `RuntimeError`, attributed to the module and distinguished from assertion detection.
- A mutant exceeding its deadline is retried once with an extended deadline. Repeated expiry is `Timeout`; a completed retry supplies the verdict.
- A selected test disappearing, becoming skipped, or lacking a success record cannot establish `Survived`. Reproducible mutant effects are distinguished from runner/protocol failures.
- Changed inputs, stale records, or broken communication retain established partial results and leave unresolved candidates incomplete. They cannot produce a passing gate or full completion.

### Apply strict detection only when requested

- A fully completed run with survivors/unreached mutants exits successfully by default while showing those states separately from execution completion.
- `--fail-on-undetected` fails a completed run with `Survived` or `NoCoverage`, separately from configuration/baseline/runner errors.
- Zero candidates produces a count of zero and no score, never a claim of 100% detection.

### Preserve existing work

- Duplicate names and concurrent tests are not substituted for one another. If reach cannot be isolated by test, expand execution and disclose its precision.
- Completion/interruption does not save mutated bytes into source, tests, configuration, or scenario history. Interruptions stop only owned processes and report incompletion.
- JSON stdout remains one result, including partial results; configuration/test logs go to stderr.
- Existing CLI, exit-code, configuration, JSON, and testkit contracts remain compatible.

## Candidate requirements

Inputs are project, include/exclude patterns, and excluded families. Output contains stable ID, family, original location, original/replacement text, and exclusion reason where applicable.

Supported source extensions are JS/JSX/TS/TSX and their module variants: `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`. Exclude dependencies, declaration-only files, test bodies, and collected Vitest configuration/setup/globalSetup. Enumerate even unimported selected files.

The 18 families are `ArithmeticOperator`, `ArrayDeclaration`, `ArrowFunction`, `AssignmentOperator`, `BlockStatement`, `BooleanLiteral`, `CallExpression`, `ConditionalExpression`, `EqualityOperator`, `FnValue`, `LogicalOperator`, `MethodExpression`, `ObjectLiteral`, `OptionalChaining`, `Regex`, `StringLiteral`, `UnaryOperator`, and `UpdateOperator`. Users can exclude individual families.

Candidates are runtime expressions/statements. They must not change type syntax, import/export specifiers, or directive semantics. Deduplicate identical range/replacement edits. Exclude with a reason any placement whose inactive state cannot preserve evaluation count, short-circuiting, or call semantics. The same input/configuration/rule version yields the same candidate identities and order. Positions refer to original text and remain correct for Unicode and emoji.

Invalid patterns/families, outside-root paths, zero selected files, unreadable source, and syntax failure are errors, distinct from selected files with zero candidates. Never execute a guessed replacement for unparsed source.

## Execution requirements

Perform an ordinary baseline and an inactive instrumented probe, matching runnable test inventory and outcomes before trials. Honor project aliases/environment/setup and disable snapshot updating. Prefer per-test reach; broaden initialization/concurrent/ambiguous observations to their file or all selected tests and expose the precision. A final test failure may end an ordinary mutant trial early.

`Survived` requires reach plus success of every selected runnable test. Skip/todo, missing registration, or absent evidence is not success. A timeout gets one extended retry; runtime/module failures get one same-condition confirmation. At most two attempts are allowed. A retry that only establishes a different failure class leaves the run unstable/incomplete. A measured hit-limit stop may be classified as timeout without retry.

Startup/communication failures are not mutant runtime errors. Changes to source, configuration, tests, or acquired dependencies during execution prevent completion against old reach records.

## Results and gate

| State | Meaning | Score treatment |
|---|---|---|
| `Killed` | A selected test fails under the mutant | Detected |
| `Survived` | Every selected test executes successfully | Undetected |
| `NoCoverage` | No reach to the instrumented execution unit | Undetected |
| `Timeout` | Confirmed deadline or hit-limit stop | Detected, separately identified |
| `RuntimeError` | Confirmed non-test/module failure under the mutant | Detected, separately identified |
| `Ignored` | Explicitly excluded, including unsafe placement | Outside denominator |
| `Pending` | Interrupted or unconfirmed due to execution failure | No final score/gate |

No denominator means no score. An exclusion is not an equivalence approval. Report whether compiler typechecking ran. The default displays evidence for investigation; a strict completed gate fails undetected mutants while preserving exclusion counts/reasons. Incomplete execution takes precedence over quality results.

Unsupported Vitest versions/APIs, Browser Mode, unselected multiple projects, or missing runtime dependencies fail with actionable reasons. Scenario snapshots must not be reinterpreted as mutation cache history.

## Data, security, and evidence

One run owns source versions/fingerprints, candidate inventory, test identities, baseline reach/precision/timing, attempts, final judgments, and diagnostics. Multiple attempts still count as one mutant. Records from different runs cannot be mixed. This foundational path does not itself reuse earlier execution results; the [workflow design](../mutation-workflows/design.md) specifies opt-in reuse.

Paths, source excerpts, test names, and errors may contain project information. There is no automatic external report upload or package download. Tests/configuration execute with local user permissions and remain responsible for their own filesystem/network effects. Temporary data and owned processes are cleaned without stopping unrelated runs.

Practical usefulness requires recording real missed-change classifications, developer-chosen test/assertion improvements, and the subsequent results at a fixed project revision. This KPI has not been established by passing fixtures. Performance targets follow measurement; [implementation status](../../implementation-status.md) separates existing evidence from remaining real-project acceptance.
