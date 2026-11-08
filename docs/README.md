# errspec documentation

These specifications record errspec's purpose, behavior and implementation
boundaries. The full implementation plan and capability scope are approved;
local implementation and practical-trial evidence are recorded separately from
required remote validation. See [implementation status](products/errspec/implementation-status.md)
for verified work, blocked CI and remaining acceptance.

Read [the Product](products/errspec/product.md) first, then the feature's
Requirements for observable behavior and Design for the implementation choices.

| Feature | Requirements | Design |
| --- | --- | --- |
| Failure contracts and scenario verification | [Requirements](products/errspec/features/failure-verification/requirements.md) | [Design](products/errspec/features/failure-verification/design.md) |
| Error discovery and propagation | [Requirements](products/errspec/features/discovery/requirements.md) | [Design](products/errspec/features/discovery/design.md) |
| Failure paths from code to scenarios | [Requirements](products/errspec/features/failure-paths/requirements.md) | [Design](products/errspec/features/failure-paths/design.md) |
| Existing-test error coverage | [Requirements](products/errspec/features/test-coverage/requirements.md) | [Design](products/errspec/features/test-coverage/design.md) |
| Mutation generation, execution and verdicts | [Requirements](products/errspec/features/mutation-testing/requirements.md) | [Design](products/errspec/features/mutation-testing/design.md) |
| Diff selection, reuse, adapters and workflows | [Requirements](products/errspec/features/mutation-workflows/requirements.md) | [Design](products/errspec/features/mutation-workflows/design.md) |
| Failure atomicity under injected faults | [Requirements](products/errspec/features/failure-atomicity/requirements.md) | [Design](products/errspec/features/failure-atomicity/design.md) |
| TypeScript 6/7 analysis backends | [Requirements](products/errspec/features/typescript-compatibility/requirements.md) | [Design](products/errspec/features/typescript-compatibility/design.md) |
| Distribution and initial generation | [Requirements](products/errspec/features/distribution/requirements.md) | [Design](products/errspec/features/distribution/design.md) |

The [fixed mutator capability comparison](products/errspec/mutator-coverage.md)
separates the inclusion target, implemented behavior and validation limits.
Errspec preserves its existing CLI, exit codes, native JSON and testkit contracts;
functional inclusion does not require copying mutator's public interface.

## Engineering and integration

- [Specification workflow](engineering/guidelines/specification-workflow.md)
- [Rust ownership and snapshot tests](engineering/guidelines/rust-code.md)
- [Node integration test support](engineering/guidelines/integration-tests.md)
- [Mutation execution and change boundaries](engineering/mutation-flow.md)
- [Preparing distribution packages](engineering/guidelines/distribution.md)
- [Failure-scenario testkit](../packages/testkit/README.md)
- [Core API and mutation configuration](../packages/core/README.md)
- [Vitest entry](../packages/vitest/README.md), [Jest entry](../packages/jest/README.md), and [Playwright fixtures](../packages/playwright/README.md)
- [Independent compiler facts and worker API](../packages/typecheck/README.md)
- [GitHub Action](integrations/github-action.md)
- [Bundled Agent Skills and initialization](integrations/agent-skills.md)
- [KARTE source-free failure-verification trial](products/errspec/practical-failure-trial.md)
- [Journey full/repeat comparison](engineering/benchmarks/journey-2026-11-05.md) and [scope/test-improvement trial](products/errspec/practical-mutation-trial.md)
- [Bounded source batching: reproducible throughput and end-to-end limits](engineering/benchmarks/source-batching-2026-11-06.md)
- [Mutation concurrency: time, process-tree RSS and coordinator extraction](engineering/benchmarks/mutation-concurrency-2026-11-08.md)

English is the maintained documentation language. Git and PRs retain earlier
wording, decisions and validation history. No specification bootstrap is required.
These Product/Requirements/Design documents specify errspec itself; consumer
failure scenarios in `errspec.json` specify the application being verified.
Neither a document nor a passing check automatically proves human agreement or
complete application correctness.
