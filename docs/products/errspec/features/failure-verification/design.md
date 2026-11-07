# Matching types, scenarios, and execution evidence

This design implements [Product](../../product.md) and the [requirements](requirements.md). The value is the correspondence between type variants, registered behavior, tests, and execution evidence. Agent JSON carries diagnostics; fault injection and mutation testing alone are not claimed as distinctive inventions.

## Responsibility boundaries

CLI orchestration, acquisition and rendering live under `src/bin/errspec/`,
separate from the public library. Existing module names and child relationships
remain unchanged, preserving public APIs, execution order and report contracts.

| Module | Responsibility |
|---|---|
| `analysis_backend` | Select the backend from explicit options and installed versions; own official-API requests, responses, and processes |
| `tsrs_adapter` | Read tsconfig/diagnostics, resolve exported symbols and finite tags, provide AST traversal, text, and positions |
| `model` / `config` | External data/enums and immutable validated configuration |
| `contract` | Compare type tags and registrations using acquired file-existence facts |
| `snapshot` | Build inventories from registrations; compare added, removed, and changed scenarios |
| `evidence` | Match resolved file identity plus full test name and judge test state |
| `vitest` / `bun` | Convert JSON/JUnit into common `RunEvidence` |
| `observation` | Validate helper records and compare them with registration, tests, and required observations |
| `observation_io` | Read observation files in the required order and within limits |
| `scenario_source` | Acquire definite pre-execution mismatches from registered test source |
| `input` | Read JSON, check files before execution, resolve paths after execution |
| `runner` | Own subprocesses, environment, runner reports, temporary observation files, and current run ID |
| `verification` | Sequence post-run path resolution, test matching, observation loading/matching, and partial results |
| `report` | Convert judgments into the external report |
| `main` / `cli` / `request` | Convert arguments into parser-independent internal requests and runner choices |
| `app` | Sequence configuration, shared analysis thread, registration, baseline, and verification |
| `output` | Render text/JSON, separate stdout/stderr, select exit status |
| `packages/testkit` | Record fault execution and value comparison inside JS runners; maintain copyable `index.ts` as the source and generate `index.mjs` plus `index.d.mts` |

The testkit build script uses pinned TypeScript 6.0.3 to generate JavaScript and
public declarations from `index.ts`. Check and Distribution CI compare both
committed artifacts with that output before verification or packaging. The source
remains self-contained for vendoring; neither distribution entry needs a consumer
build. The same observation and atomicity cases run against the source and the
generated entry, with strict consumer checks for declarations and copied source.

TypeScript/JavaScript executes in its native Bun/Vitest runner. tsrs or the project's official TypeScript 6/7 API acquires semantic facts; common Rust judgments compare them with registrations and evidence. Backend parity and selection are specified in [TypeScript compatibility](../typescript-compatibility/design.md). There is no second TypeScript type-system implementation. tsrs internal types stay inside its adapter; no additional `oxc_parser` is introduced.

The tsrs adapter substitutes an unspecified module-resolution option for removed `moduleResolution: node10` before creating its Program. tsrs resolves unspecified CommonJS using `bundler`; this avoids stopping only for TS5108. The report records the substitution in `assumptions` without changing `ok`. Other removed options are not substituted because their resolution behavior can differ.

Shared code follows the [Rust guide](../../../../engineering/guidelines/rust-code.md). Adapter traversal/position rules serve both discovery and source checks. `Location` belongs to the shared model and remains re-exported as `discovery::Location`. Git revision and Cargo.lock pin compiler dependencies. The arena lives for a one-shot process; this workflow does not expose a persistent analysis service. Rust alone does not establish speed or soundness.

## Registration and scenario identity

Let E be type-derived tags and C registered tags. E−C is missing registration; C−E is stale registration. A scenario is identified by `(contract id, scenario id)`; multiple scenarios may share a tag. Equality of tag sets does not prove coverage of payloads, time, or all failure paths.

Configuration v2 requires fault IDs and at least one assertion ID per case. An explicitly empty `faults` array permits an input-only scenario; omitting `faults` remains invalid. The matching test must declare `withoutFaults: true`. Without that declaration, zero faults fail; with it, calling `fault` fails. Both sides must agree. The observation protocol stays v1. `expect` is descriptive text, not executable code. Types are explicitly exported/registered; Result type arguments are not inferred by name.

The helper's stable test name is `errspec ` followed by a JSON array `[contract id, scenario id]`. JSON prevents separator collisions. An optional nonempty single-line `description` follows one space. The helper acquires its file from `import.meta.url`; Rust canonicalizes it.

For v2, a runner's full name may contain the stable name after whitespace-delimited `describe` prefixes and before a whitespace-delimited description. Bun's nested JUnit suites and Vitest's full names join suite titles with spaces. Only one candidate may match. Multiple suite registrations are ambiguous. Because the helper does not know suite titles, its observed name must equal the full name's suffix starting at the stable identity. Legacy v1 names retain exact matching.

Descriptions do not enter configuration or snapshots and do not produce `changedScenarios`/EC014. Runner evidence, rather than helper self-reporting alone, establishes test success. Duplicate file/name identities or repeated executions of one scenario are rejected. Retry/repeat and browser execution remain outside this failure-verification runner contract; distinct concurrent scenario IDs receive independent scopes and files.

## Observation lifecycle

1. The CLI creates a new temporary directory and run ID.
2. The helper starts a scenario with a `running` record.
3. A fault wrapper increments calls, then completed on resolve/reject.
4. `check` uses `deepStrictEqual` and records passed/failed. Catching its error does not clear failure.
5. At body completion, the helper verifies required calls/completions/assertions and writes the final state.
6. After runner exit, Rust matches the runner report and observations.

Each scenario execution uses a UUID filename, written by temporary-file rename. Interrupted `running` records, leftover temporary files, and broken JSON cannot pass. Limits are 1 MiB per file, 32 MiB total, and 10,000 files; violations produce EC_EVIDENCE. The run ID prevents accidental mixing, not malicious forgery.

Success requires all tags registered, one current successful test per scenario, and all of the following:

- Matching run, contract, scenario, tag, test file, and name.
- Exactly one observation with state `passed`.
- Every mandatory fault called at least once with all calls complete, or no observed faults for an explicitly fault-free case.
- Every mandatory assertion passed.
- No unknown scenario, fault, or assertion IDs.
- Successful runner report and process exit.

Fault completion does not prove the semantic meaning of the injected value. Expected values, observed state, and specification sufficiency remain review responsibilities. The process, helper, and tests are trusted; arbitrary JS or activity outside the observation interval is not proved.

## States, baseline, and compatibility

The report uses v2. Legacy configuration v1 does not require observations and reports `not_required`. `check` leaves them `not_run`; `verify` performs execution matching. Rust enums distinguish `TestStatus`, `ObservationStatus`, `ScenarioState`, and `AssertionStatus`. Unknown runner states retain their original text and never become success. Test and observation statuses stay separate because a test can pass while a required assertion is missing.

Configuration, report, snapshot, and observation protocol versions are independent. New snapshots use v2; old v1 snapshots remain readable. Existing `analysisComplete` also becomes false for runner/report failures. Its meaning is preserved rather than redefined as strictly static completion.

A v2 snapshot stores tags and scenario ID, tag, expected behavior, test reference, and required observations. Removal produces EC013 even if another scenario still represents the tag. A changed existing scenario appears in `changedScenarios`. `--strict-baseline` adds EC014 per changed scenario to `check`/`verify`, with exit 1 if no infrastructure/configuration error supersedes it. Additions alone do not fail; existing EC007/EC013 removal rules remain.

Strict comparison requires `--baseline`, configuration v2, and snapshot v2 scenario history for every contract. Missing CLI baseline is an argument error; incompatible configuration or use with `snapshot` is EC_CONFIG; missing historical data is EC_BASELINE, exit 2. Snapshot conflicts are rejected before writing. Valid EC014 differences do not prevent `verify` from running tests/evidence, so both changes and current results are returned. Corrupt history or invalid configuration prevents execution. Old snapshots cannot identify pre-migration scenario deletion. A changed source-definition reference invalidates the baseline instead of assuming type identity.

The caller controls baseline provenance. Creating/replacing a snapshot does not prove approval; CI must obtain its comparison source separately from the proposed change.

## Owned input and pure judgments

Keep one Cargo package with explicit modules. CLI types exist only at conversion boundaries. Internal request variants distinguish contract operations, discovery, and generation. A shared one-shot analysis thread uses a 32 MiB stack, distinguishes spawn failure from panic, maps errors to existing command diagnostics, and never returns tsrs handles across the boundary.

External processing acquires files, compiler facts, subprocess outputs, and temporary resources as owned values. Pure judgments determine registration gaps, test matching, observation sufficiency, and snapshot changes without touching files, processes, or a partially assembled report. Final conversion produces the external report once. Snapshots derive directly from registrations, independently of test execution and report rendering.

Registration results borrow validated configuration and contain type tags and registered cases or missing tags. Test matching returns file/full-name keyed results; observation matching consumes those results. Each stage preserves its diagnostic order. A report-read success followed by observation-read failure retains test results and adds the observation error. A runner-report read failure retains the existing unexecuted display; this distinction does not newly define whether a subprocess physically started.

`TryFrom<Config>` creates `ValidatedConfig` with private fields and immutable contracts/cases. Validated types expose neither `Deserialize` nor mutable references. An enum separates legacy and observation-enabled cases; the latter always has scenario identity, test reference, and requirements. Construction checks existing empty/duplicate/version rules in the same order and computes the stable test name once. Discovery/generation do not manufacture empty contract configurations to reach analysis.

`ValidatedObservations` holds format/run-validated records, including an empty collection's run ID. In-memory and filesystem validation use the same `RunContext`. Matching rejects a mismatched current run ID before returning judgments, without mutating registration or test inputs. Raw `Observation` cannot enter matching directly. The type establishes record validity, not scenario success, business meaning, or authentication.

Test results distinguish passed, failed, unexecuted, missing, and duplicate. `pending`, `skipped`, `todo`, and unknown Vitest strings remain expressible in existing output. Unknown Bun report structures remain rejected. Recoverable file/input/runner failures use `Result`; ordinary failures do not become panics.

## Execution ownership and ordering

The runner has no dependency on contract verdicts. It returns common test/process results and an owning session whose private fields retain the observation directory and run ID. Files remain until verification finishes, then drop cleanup removes them. Startup or report-read failure also releases temporary resources.

Verification first matches returned tests, then loads observations through the session, resolves observation paths, and matches them. Loading observations inside runner startup would change partial-result behavior and is avoided. Pre-test file existence and post-test evidence identity remain distinct acquisition stages. Unresolved paths cannot pass.

Argument conversion preserves validation order: scan/init baseline options fail before execution; a verify runner/executable mismatch is diagnosed after the existing configuration, analysis, and baseline stages. Enum/set refactors preserve visible ordering. Runner logs remain on stderr and never contaminate JSON stdout. Explicit executable resolution and PATH lookup for an unspecified Bun remain distinct.

Before starting Bun, query that same executable's version. Versions through 1.2.18 fail with EC_RUNNER because JUnit omits file identity. An unreadable/unparseable version retains ordinary execution and strict report validation. Preflight rejection reports unexecuted tests and `analysisComplete: false`, matching report-read failure.

Registration gaps can still allow verify/snapshot to proceed under existing rules; invalid configuration or incomplete analysis stops them. Snapshot creation may write an inventory despite registration gaps but never overwrites. Default behavior reports expected-behavior/observation changes; strict mode fails them. Changing these contracts requires a separate requirement change.

These boundaries preserve CLI arguments/defaults/exit codes, configuration v1/v2, report v2, observation v1, snapshot compatibility, testkit API, diagnostic codes/order, omitted fields, stream routing, and generated JSON ordering/escaping. They do not create a persistent JS semantic API, FFI, generic DI/plugin framework, async runtime, or multiple crates for this workflow. Separate mutation APIs have their own [design](../mutation-workflows/design.md).

## Static scenario matching

After type analysis, inspect registered v2 test files already in the tsconfig project. Do not add external tests to analysis or diagnose them merely for exclusion, because doing so could introduce test type errors that stop existing analysis.

Resolve `scenarioTest` through the called signature's declaration: its name must be `scenarioTest` and its module must export `ScenarioScope`. Resolve `fault`/`check` as methods declared on `ScenarioScope`; matching spelling alone is insufficient.

Read object-literal definitions only. Extract finite string-literal sets from each property's initializer type, not the contextually widened whole object. Unbounded strings or unresolved spreads are skipped. Direct calls inside the fourth-argument body supply fault/check IDs by the same rule; helpers outside that body remain runtime-only.

EC015 covers unregistered contract/scenario combinations, wrong test file, wrong tag, and disagreement between `withoutFaults` and empty configured faults. EC016 covers concrete fault/assertion IDs absent from required IDs. Missing required IDs are not statically diagnosed because a helper could supply them. These conditions already fail runtime matching. `verify` reports them while still executing tests and matching observations.

## Explicit registered-file execution

`verify --only-contract-tests` collects case test paths, resolves them from the configuration directory, deduplicates them, and passes them as runner file arguments. Empty selection is EC_CONFIG. Omitting the option supplies no extra file arguments and preserves full execution.

## Validation and future boundaries

The extraction sequence was CLI/execution/display boundaries, validated inputs, then pure judgments/report conversion. Each step must preserve external behavior and be independently reviewable. Shared Rust/JS fixtures use independent expectations for JSON-array names, Unicode/quotes, states, and counters. Testkit remains a small API without an assertion DSL or independent Result implementation.

Validation includes Rust CLI analysis, report fixtures and actual Bun/Vitest runs, JS testkit tests, observation limits/stale IDs/corruption, incomplete and caught-failed assertions, baseline removals/changes, and representative CLI equivalence with normalization limited to environmental paths. Filesystem tests cover creation/deletion timing, symlink identity, partial reads, and diagnostic order. Required commands live in the [Rust guide](../../../../engineering/guidelines/rust-code.md), with current acceptance links in [implementation status](../../implementation-status.md).

Changing `analysisComplete`, timeouts, or log-retention limits is separate user-visible work. Persistent compiler resource management cannot reuse one-shot assumptions. Broader Result adapters, semantic fault observation, and stronger propagation need explicit evidence and requirements. Mutation discovery/caching are now specified separately; they must not silently expand the guarantees of this scenario verifier. Unknown relationships must not justify skipping required tests.
