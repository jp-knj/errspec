# Full workflow implementation status

The approved implementation plan covers every row of the fixed mutator 0.2.0
capability matrix plus errspec's existing failure verification. The user approved
implementation and validation in small PRs, including merging validated changes.
That scope approval does not imply that unfinished capabilities are implemented,
or that every subsequent design detail has received a separate human review.

Comparison revision: `025870640d46e93e97285d67e4c265074cf8b295`.
Completion requires executable acceptance coverage for every requirement, package
validation on Linux x64 and both macOS architectures, and the KARTE practical
trial. No npm/crates publication or production deployment is included.

## Acceptance traceability

| Requirement / accepted behavior | Implementation and executable evidence | Remaining work |
| --- | --- | --- |
| Existing mutation generation, baseline, reachability, retries and verdicts | `src/mutation.rs`, `src/mutation_source.rs`, `src/mutate.rs`; `scripts/demo-mutation.mjs`, `tests/mutation-emitter.test.mjs`, `tests/mutation-process.test.mjs`; `tests/mutation-baseline.test.mjs` preserves module/assertion causes and tolerates equivalent file-enumeration orders | Preserve these checks throughout |
| TS 6/7 compile errors and sound optional chaining exclusions | `src/mutate.rs`, embedded compiler adapter; `tests/mutation-typecheck.test.mjs` | Public typecheck package |
| Weak mutation distinguishes reached but not infected | `src/mutate.rs`, driver instrumentation; `tests/mutation-weak.test.mjs` | Additional runner integration |
| Stable mutant IDs, result/coverage reuse, conservative invalidation, removed killing tests | Stable IDs plus opt-in exact-input result reuse: `src/mutation/cache.rs`, `src/mutation_cache.rs`, `tests/mutation_cache.rs`, `tests/mutation-cache.test.mjs`; current baselines and first-attempt assertion outcomes | Phase 2: coverage reuse, cross-checkout and scope-aware reuse, plugin invalidation; initial cache remains strict and revalidates position-only edits |
| Git node/scope selection, uncollected changed-test warning, full dry run, experimental callgraph | `src/mutation/selection.rs` and bounded Git acquisition; `tests/mutation-selection.test.mjs` and `tests/mutation_selection.rs` cover line/scope selection, new/staged/untracked/deleted tests, unusual paths, monorepo roots, API equivalence and concurrent input changes | Phase 2: persistent cache integration, explicit full dry run and experimental callgraph |
| Mutant IDs, deterministic per-line sample, time budget, acceptance with reasons, thresholds and critical scopes | `src/mutation/policy.rs`; `tests/mutation-workflows.test.mjs` | Pending candidates leave the overall score unknown and prevent full-score/undetected gate success; keep partial runs distinct across added execution modes |
| Isolated parallel/sharded execution and reusable Vitest sessions | Opt-in `reuseSession` preserves isolated worker and request evidence; `tests/mutation-session.test.mjs`, repeated mutation acceptance and process tests cover state/mocks, timeouts, stale events, cleanup and fallback | Phase 2: parallel/sharded execution; default concurrency 1 |
| Owned worker termination and orphan recovery with PID-reuse protection | Current invocation termination: `tests/mutation-process.test.mjs`; config loading: `tests/mutation-options.test.mjs` | Phase 2: persisted ownership and orphan recovery |
| Arid, callee, disable/Stryker exclusions and custom mutator/ignorer plugins | Built-in equivalent exclusions only | Phase 3: plugin boundary, reasons, disable controls and invalidation |
| Not detected/infected/reached and child-process warning | Native statuses and optional infection facts exist | Phase 3: explanation reports and warnings |
| Compatible JSON/HTML, agent/simplify, select-tests, complete kill matrix, annotations | Native text/JSON report exists | Phase 3: public format/select APIs, schemas and full matrix acquisition |
| Vitest 5, Vite+, runner selection | Vitest 5 and explicit package/config selection | Phase 4: reusable sessions, Vite+ fixture and automatic selection |
| Jest 30 native ESM; Playwright/Vite fixture and multiple projects | Not implemented | Phase 4: adapters sharing native verdicts and real fixtures |
| Mutation config, JSON Schema, explicit precedence, negative flags | `src/mutation/options.rs`, `src/mutation_config.rs`, `packages/core`; `tests/mutation-options.test.mjs` | Extend schema/options alongside subsequent capabilities |
| `defineConfig`, `runMutation`, same CLI/API verdicts without argument reconstruction | `src/mutation_api.rs`, `packages/core`; equivalence, errors, interruption and strict declarations in `tests/mutation-options.test.mjs` | Formatter, select, plugin APIs and runner/typecheck package separation |
| Native npm packages and offline installation on three platforms | `scripts/pack.mjs`, `scripts/package-smoke.mjs`, Distribution workflow; now includes core API discovery | Add new packages and runner fixtures as implemented |
| GitHub Action, base fetch and cache save on failure | Not implemented | Phase 5 |
| Agent Skills and init-skill | Not implemented | Phase 5 |
| Existing error contracts, discovery, TS backends, coverage and failure atomicity | Rust tests, Node testkit/adapter tests, `demo-*.mjs`, Valibot pilot | Regression checks plus KARTE trials |
| KARTE mutation and failure-verification trial; comparison report | Earlier exploratory trials informed scope; no full-plan trial completed | Phase 5: retain lockfiles; journey/usecases full/repeat/diff; obel vcs and agent-studio coverage/fault checks; record time, useful findings, noise and investigation costs |
| English documentation and removal of temporary sdev tooling | This status and public API guide are English | Phase 5: reconcile final specs, translate remaining documents and remove temporary configuration/guidance |

## Delivery and estimates

Phase 1 introduces shared options, configuration and the initial public execution
API. Its acceptance checks are part of Check CI; offline installed API execution
is part of three-platform Distribution CI. Later features stay explicitly pending.

The initial estimate remains 30–50 person-days (6–10 weeks for one person with
AI assistance), including review, CI and practical validation. Configuration/API
integration does not provide evidence for reducing the high-risk runtime, cache,
runner and KARTE estimates. Re-estimate those stages after their first measured
end-to-end fixtures; do not infer calendar completion from this partial delivery.
