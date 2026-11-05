# Capability coverage against mutator 0.2.0

[Product](product.md) requires the fixed reference's user capabilities together with errspec's error-type and failure-scenario verification. This table describes implementation boundaries, not a declaration that every acceptance/release condition has passed. Follow the [workflow requirements](features/mutation-workflows/requirements.md), [design](features/mutation-workflows/design.md), and [acceptance/evidence index](implementation-status.md).

The reference is mutator 0.2.0 at `025870640d46e93e97285d67e4c265074cf8b295`: [README](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/README.md), [CLI package](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/cli/package.json), [options](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/cli/src/options.ts), and [mutators](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/core/src/mutators.ts). These are primary implementation references, not code to copy blindly.

Inclusion means equivalent usable capability, not identical commands, configuration, JSON, exit codes, or plugin APIs. Existing errspec CLI/native JSON/configuration/testkit contracts remain. Separate feature scopes do not weaken the full product goal. New performance or correctness guarantees require evidence rather than a checked capability row.

## Generation, execution, and reuse

| Fixed-reference capability | errspec implementation and boundary |
|---|---|
| AST instrumentation, switching, formatting, source maps | tsrs runtime AST and MagicString in-memory emission; [generation design](features/mutation-testing/design.md) specifies safe placement. |
| 18 built-in mutator families | All families below, with explicit safe-placement/exclusion reasons. |
| Baseline, per-test reach, reached-test selection, initialization mutants | Ordinary baseline plus inactive probe; concurrent/static reach expands to file/suite when necessary. |
| In-memory Vitest/Vite transform and reusable sessions | Opt-in `--reuse-session`, default fresh. globalSetup or missing public APIs falls back fresh with engine.sessionReuse reason. |
| Stable identity and source/test/dependency/settings/environment/plugin-aware cache | Exact result/probe reuse is opt-in. Ordinary baseline always runs. Explicit relocation supports path-independent checkout copies; scope reuse uses validated function/entry cohorts and full current probes after edits. Comments, opaque calls, missing entries, and unknown inputs broaden revalidation. Plugin module/dependency/settings/data changes invalidate reuse. Partial probe refresh is a [deferred optimization](implementation-status.md#probe-reuse-audit), not a missing user feature. Mutation cache remains separate from scenario snapshots. |
| Git node/scope selection, removed killing tests, uncollected changed tests | `--since`, `--diff-unit node/scope`; changed/deleted tests and outside-inventory edits broaden revalidation. Cache validates current covering/selected/killing-test references. Uncollected changed tests warn. |
| Explicit mutant selection | `--only`; unknown IDs fail. |
| Forced full probe | `--full-dry-run`/fullDryRun, independently of result reuse; negative flag clears configuration. |
| Experimental callgraph pruning of old survivors | `--cache --cache-scope --experimental-callgraph`, default off; bidirectional acquired static calls/resolved imports. Unknown calls/entries revalidate. Warn about unmodeled through-test value flow and disclose omissions. |
| Weak mutation, infection-based narrowing, disablement | Opt-in comparison/arithmetic acquisition distinguishes not infected. Unsupported/static/concurrent cases broaden to covering tests. |
| Compiler checks and CompileError | Opt-in `--typecheck`/negative flag, tested with TS 5.9.3/6.0.3/7.0.2. Compare mutant-only diagnostic additions. |
| Type-proved optional-chain exclusions | Require strictNullChecks and noUncheckedIndexedAccess plus applicable receiver types; retain reasons. |
| Arid logging heuristics/custom callees/disablement | Opt-in `--arid`, default off; callee patterns replace defaults. Logging names do not establish side-effect equivalence. |
| Disable and Stryker comments | errspec-/mutator- line/next-line/region directives and Stryker disable/restore with family/reason/restore handling. Literal/template/regex/JSX false directives do not exclude. |
| Reasoned acceptance and stale acceptance reporting | `accept --reason`, `errspec.accepted.json`; current verdict/gate applies acceptance separately. |
| Custom module/package mutators/ignorers and cache invalidation | `plugins`, `--plugin`, typed defineMutator/defineIgnorer/definePlugin. Rust validates IDs/ranges/placement/reparse/reason precedence. Acquired import closure/settings/declared inputs bind deterministic cache; unknown inputs stay fresh. |
| Parallel trials and splitting long runs | `-j`/concurrency uses independent owned workers with matching probes, default 1. `--shard INDEX/TOTAL` deterministically partitions post-Git/only candidates. Scores/gates/budgets remain per invocation/shard. |
| Deterministic per-line sample and time budget | `--mutants-per-line`, `--time-budget`; unstarted candidates stay reasoned Pending with unknown overall score. |
| Hit/stall/total limits and bounded confirmation | Shared native verdicts; at most one retry per mutant, extended timeout or same-condition RuntimeError confirmation. |
| Signal cleanup and orphan recovery | Native supervisor owns runner groups, detects parent death, verifies recovery capabilities, and never signals saved PIDs directly. Unix endpoint restrictions retain parent monitoring and are reported. Deliberately detached descendants remain outside ownership. |

## Built-in families

| Category | Families |
|---|---|
| Operators | ArithmeticOperator, AssignmentOperator, EqualityOperator, LogicalOperator, UnaryOperator, UpdateOperator |
| Conditions and bodies | ConditionalExpression, BlockStatement |
| Literals, arrays, and objects | BooleanLiteral, StringLiteral, ArrayDeclaration, ObjectLiteral |
| Function values | ArrowFunction, FnValue |
| Calls and access | CallExpression, MethodExpression, OptionalChaining |
| Regular expressions | Regex |

Specific edits and inactive-state rules are in the [mutation requirements](features/mutation-testing/requirements.md) and [design](features/mutation-testing/design.md).

## Runners, reports, and quality decisions

| Fixed-reference capability | errspec implementation and boundary |
|---|---|
| Vitest 5/Vite+, automatic and explicit selection | auto/vitest/vite-plus/jest/playwright through CLI/configuration/API. Real Vite+ 1.5.0 fixture uses bundled Vitest 5.1.0 without root-level Vitest. |
| Experimental Jest 30 native ESM | Real Jest 30.5.2 tests cover TS, custom async ESM transforms, duplicate IDs, skip/concurrent, parallel workers, and API parity. CommonJS and Jest multi-project configs are outside this adapter contract. |
| Experimental Playwright/Vite, fixture extension, multiple projects | Real Chromium with Playwright 1.64.1/Vite 8.3.4 covers projects, fixture extension, navigation/frames, project selection, parallel probes, missing fixture, and real descendant cleanup. Vite server ownership and relative navigation are required; browser WebWorkers/custom servers are excluded. Unknown external inputs prevent cache reuse. |
| Text, compatible JSON/HTML, output directory | Preserve default text and native `--format json`. `--reporter text/json/html` and `--report-dir` add MTE schema v2 and offline HTML. Real-report schema/viewer tests exercise output. |
| Not detected / infected / reached and child-process risks | Distinct gap categories. Actual Node launch facts attribute executable/relevant relative script closure to tests. No raw argv/env output, generic warning, dynamic/package/alias/shell-resolution claim, or automatic coverage promotion. |
| Agent JSON with source/tests/fixes | `--reporter agent`/formatReport attaches validated available function source, covering tests, and assertion suggestions. Saved-report formatting does not refresh verdicts or prove complete old source identity. Stdout stays one JSON result, including under GitHub Actions. |
| Simplify/removal candidates | Covered Survived deletion candidates with location/function/key/edit. Exclude NoCoverage, Ignored, and possible child-execution cases. |
| Ranked related tests as files/JSON | `select-tests --since`/selectTests uses current baseline/probe coverage; historical kills are hints. Changed/deleted tests/code, helper/module changes, or insufficient observation fall back to current collected files. Final inventory is validated. |
| Complete kill matrix and no-kill/subsumed/duplicate relations | Execute all covering tests; complete rows alone establish relations. No first-detection/scope row reuse; compatible probe reuse remains. Fresh matrix rows each run; no safe-deletion proof. |
| High/low/break thresholds and fail-on-survived | Dedicated quality gate, exit 1 for completed failure; baseline/infrastructure errors exit 2. |
| Critical globs/comments require detection or approval | `--critical` and preceding errspec-critical/mutator-critical comments, with reasoned acceptance. |
| GitHub warning annotations on changed lines | Shared annotation formatter and Action emit escaped file/line warnings; actual Git/GitHub fixtures cover them. |

## Configuration, distribution, and agent workflows

| Fixed-reference capability | errspec implementation and boundary |
|---|---|
| ts/mts/mjs/js/json config, defineConfig/schema, precedence and negative flags | `errspec.mutation.config.{ts,mts,mjs,js,json}`, explicit CLI/API over validated config over defaults; arrays replace, paths retain documented roots. |
| Programmatic execution/formatting/plugin APIs and package separation | Public core runMutation/formatReport/selectTests and definition helpers use the same native JSON request/verdict path without CLI reconstruction. Runner/typecheck package delivery is tracked separately in [status](implementation-status.md). |
| CLI/adapter artifacts, Node 24 | Embedded mutation drivers require Node 24. Legacy Node 22 launcher/testkit contract remains. Packages are unpublished; artifact validation does not imply registry availability. |
| GitHub Action, base fetch, save-on-failure history, copied checkout reuse | Composite Action uses native/installed artifacts, fetches missing base commits, restores/saves official cache with always-save, and delegates fingerprints/verdicts. Shallow/failing-gate/copied-input/installed fixtures pass locally; live service and three-platform CI remain distinct evidence. |
| Agent Skills, skills-npm layout, init-skill, setup/test-loop/Playwright | Three package skills share native-embedded bytes. Preflight/identical-file preservation/explicit force/symlink boundaries/rollback retain project notes. Real mutation loop and installed native-only initialization are tested. |

## Additional errspec verification

| Capability | Interface and contract |
|---|---|
| Candidate error types, throw/rejection/Result propagation and unknowns | [scan](features/discovery/requirements.md) |
| Configuration and unfinished tests for one exported type | [init](features/distribution/requirements.md) |
| Finite tags, case registration, definite source mismatches | [check](features/failure-verification/requirements.md); verify shares checks |
| Current Bun/Vitest tests, fault completion, mandatory assertions | [verify and testkit](features/failure-verification/design.md) |
| Saved scenarios, additions/removals/changes, strict baseline | [snapshot/baseline](features/failure-verification/requirements.md) |
| Observed tag assertions in existing Vitest tests | [coverage](features/test-coverage/requirements.md) |
| Explicit escaping paths and registered entry reach | [failure paths](features/failure-paths/requirements.md) |
| Fresh-fixture fault sweep and state-preservation comparison | [failure atomicity](features/failure-atomicity/requirements.md) |
| Installed TS 6/7 semantic backends | [TypeScript compatibility](features/typescript-compatibility/requirements.md) |

Every implementation claim needs its linked [acceptance evidence](implementation-status.md). Current local results do not substitute for blocked CI jobs or remaining practical evaluation.
