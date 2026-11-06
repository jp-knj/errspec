# Working on errspec

Read [the documentation index](docs/README.md), the Product and the relevant
Requirements/Design before changing behavior. The approved scope includes the
fixed mutator 0.2.0 capability matrix and errspec's failure verification. Preserve
existing CLI, exit-code, native JSON and testkit contracts.

Keep scope decisions, implementation evidence and outstanding validation distinct.
A passing local test or a document edit does not establish human approval of a
new requirement or completion of the practical trial. Do not ask again about work
already authorized in the conversation. Explain a new behavior decision when the
existing scope does not resolve it.

Use [the specification workflow](docs/engineering/guidelines/specification-workflow.md)
for documentation responsibilities and updates. Specifications are maintained in
English under `docs/products/errspec/`. Keep the root README exactly `# errspec`
followed by one newline; detailed guidance belongs in `docs/` and package READMEs.
Repository visibility changes, repository migration, npm/crates publication and
production deployment are separate future work.

## Rust development

Follow [Rust code and snapshot standards](docs/engineering/guidelines/rust-code.md)
for function/type ownership, separation of acquisition from judgment, and review
of expected results. Follow [mutation execution boundaries](docs/engineering/mutation-flow.md)
when changing compilers, runners, caches or verdicts. Compiler/runner acquisition
can remain JavaScript/TypeScript; shared planning and judgments remain native.
