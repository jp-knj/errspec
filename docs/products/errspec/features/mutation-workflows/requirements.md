# Incremental mutation workflows, reuse, and additional runners

This feature completes the approved [Product](../../product.md) scope beyond [basic generation/execution/judgment](../mutation-testing/requirements.md). The [capability matrix](../../mutator-coverage.md) fixes the reference at mutator 0.2.0 commit `025870640d46e93e97285d67e4c265074cf8b295`.

## Scope

Include Git/history selection, weak mutation, compiler checks, exclusions/acceptance/plugins, parallelism/sharding/sampling/budgets, Jest/Playwright/Vite+, reports, test selection and complete kill matrices, configuration/API/packages, GitHub Action, and Agent Skills. Existing errspec failure verification remains usable alongside mutation testing.

Reference features added after the fixed revision, arbitrary runners/browser environments, and proof of universal runtime equivalence are outside scope. Experimental adapters expose their limitations. Inclusion means usable capability, not identical CLI names, configuration, exit codes, JSON, or plugin API.

## User stories and acceptance

### Revalidate relevant changes and reuse evidence

- Identical function/test/dependency/configuration/environment/plugin inputs allow result and reach reuse, visibly distinguished from fresh execution. Location-only edits preserve mutant identity.
- Changed functions, killing tests, dependencies, settings, environment, or plugins invalidate relevant results. Deleted killing tests require revalidation.
- `--since` selects changed node/scope candidates and necessary revalidation. Changed tests absent from collection produce warnings.
- Explicit IDs report only those candidates; forced full probe ignores reach cache independently of result cache.
- Experimental callgraph selection limits revalidation of old survivors using known static relations while warning that value flow through tests may be missed.

Reuse correctness is required; partial probe refresh is not. A changed raw input may intentionally trigger a full current probe, including scope reuse. Partial refresh is a deferred performance optimization under the accepted design, as audited in [implementation status](../../implementation-status.md#probe-reuse-audit).

### Separate reach, infection, and type validity

- Weak mutation distinguishes reached-but-not-infected from unreached candidates and explains skipped trials. Disabling it executes covering tests.
- Opt-in typechecking compares original and mutated diagnostics using the project's TypeScript/tsconfig. New mutant-only diagnostics become `CompileError` and skip trials; disabling typecheck permits execution. Support TS 6/7 API paths and validated compatible versions.
- Optional-chain equivalence exclusions require applicable strict options and receiver types excluding null/undefined. Do not assume array indexing stays in range.

### Record intended exclusions and human acceptance

- Opt-in arid rules, replacement callee patterns, disable/Stryker comments, and ignorer rules retain exclusion reasons. Disabling the applicable rule restores eligible candidates.
- `accept --reason` saves mutant identity plus a nonempty developer reason and displays it in later reports. Stale acceptances are visible; acceptance without a reason is rejected.
- Module/package plugins supply custom mutators/ignorers, allow per-family exclusion, and include plugin inputs in cache invalidation.

### Select execution resources and runners

- Configured concurrency executes independent candidates with separate evidence. Deterministic sharding supports splitting a long run.
- Per-line samples are deterministic. Candidates not started because of sample/time budgets remain `Pending` with distinct reasons and cannot count as success.
- Hit/stall/total limits and a bounded retry/confirmation stop nonterminating trials and expose reasons. SIGINT/SIGTERM stop owned descendants.
- Orphan recovery requires verified ownership and must not stop unrelated processes.
- Explicit/automatic runner selection supports Vitest 5, Vite+, Jest 30 native ESM, and Playwright Test with Vite. Session reuse, ESM transformation, fixtures, and multiple browser projects retain their stated adapter limits; unsupported conditions fail clearly.

### Turn results into tests and quality decisions

- Select text, mutation-testing-elements JSON, or self-contained HTML and save the requested outputs. Preserve status, source location, and evidence; distinguish not detected, not infected, and not reached. Warn about actually observed, relevant child-process execution risks.
- Agent JSON includes gaps, available function source, covering tests, and actionable assertion suggestions. Simplify identifies covered surviving removal candidates. Machine-readable stdout stays free of logs.
- Test selection returns ranked related files/JSON based on changes and current reach.
- Complete kill matrix executes all covering tests rather than stopping at the first kill and reports no-kill, subsumed, and duplicate relationships. Reused evidence must match its acquisition mode.
- High/low/break thresholds, fail-on-survived, and critical globs/comments distinguish completed quality failure from baseline/infrastructure failure. Critical candidates require detection or reasoned acceptance.
- In GitHub Actions, undetected changed-line candidates produce file/line warning annotations.

### Integrate through configuration, APIs, artifacts, and skills

- Validate ts/mts/mjs/js/json configuration and apply explicit CLI/API properties over configuration over defaults. Negative flags can override configured true values.
- `runMutation`, formatting, and plugin-definition APIs expose the same native behavior without parsing CLI arguments. Core, runner, and typecheck entry points are available as the package plan requires.
- The Action obtains the Git base, restores/saves mutation history, and saves after a failed run so later execution can revalidate/reuse compatible evidence.
- `init-skill` installs setup, iterative test improvement, and Playwright instructions; the same skills are discoverable through skills-npm-compatible package layout.

## Compatibility and data

Existing CLI, exit-code, configuration, native JSON, testkit, and scenario-history contracts remain. Add optional fields only as required. Unexecuted work never becomes detection evidence. Invalid configuration/API/runner/history cannot pass; disclose unsupported/experimental conditions.

Mutation data associates position-independent identity, edit, state, covering/killing tests, and reasons. History associates results with source/test/dependency/configuration/environment/plugin fingerprints and acquisition mode. Human acceptance is separate identity/reason data. Reports carry scores, gates, recommendations, and observed relationships. Process records retain only necessary ownership/termination facts. Mutation history remains separate from scenario baselines.

## Services, security, and completion evidence

Local execution uses installed packages and local privileges. GitHub is required only for Action/annotations. No automatic source upload, npm publication, repository migration, or billing-setting changes are part of this implementation. Settings/plugins/tests execute trusted user code. Never signal unverified processes, and never write mutated typecheck contents to original source.

The completion criterion is traceability from every fixed-reference capability to implementation and acceptance evidence, including CLI/API/installed artifacts where applicable. [Implementation status](../../implementation-status.md) separates local evidence, blocked three-platform CI, and remaining practical work. Feature scope is approved; additional guarantees require an explicit specification change. Compiler/runner acquisition naturally remains JS/TS where appropriate, while shared validation, verdicts, gates, and planning belong to Rust.
