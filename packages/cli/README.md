# @errspec/cli

Native Rust CLI for discovering TypeScript error candidates and verifying explicitly registered failure scenarios with Bun or Vitest.

This package is being prepared for its first release. Build artifacts can be installed from local tarballs; registry installation is available only after publication.

Semantic commands automatically use an installed TypeScript 6/7 from the project or its parents, otherwise the bundled tsrs engine. The official API backend requires Node.js 22+; versions 6.0.3 and 7.0.2 are tested with Node.js 24. Use `--analysis-engine tsrs|typescript|auto`, `--typescript-package <directory>`, and `--analysis-node <executable>` to select it explicitly. The JSON `analysis` field records the engine, version, package, and reason when selection is explicit or uses the official API. Missing APIs, compiler errors, and `noCheck` produce incomplete analysis with exit 2. TypeScript 7 uses an unstable API; other releases require capability checks. These analysis flags do not apply to `mutate`. Opt into mutant typechecks with `--typecheck`, select its compiler with `--typecheck-package`/`--typecheck-project`, and disable it with `--no-typecheck`. Mutant typechecks are verified with TypeScript 5.9.3, 6.0.3 and 7.0.2.

For local tarballs, install the CLI, testkit and matching native package together. For example, on Linux x64: `npm install -D ./errspec-cli-0.1.0.tgz ./errspec-testkit-0.1.0.tgz ./errspec-cli-linux-x64-0.1.0.tgz`.

The npm launcher selects a matching native optional dependency. It performs no downloads or Rust builds in install scripts. Node.js 22 or later is required. Build targets: Linux x64 (glibc 2.35+), macOS arm64 and macOS x64. Windows and Linux musl are not included.

```sh
errspec scan --project tsconfig.json --format json
errspec init --project tsconfig.json --source src/errors.ts --export Failure --runner bun
errspec verify --runner bun --only-contract-tests
```

Install `@errspec/testkit` at the same version and your chosen runner before executing the generated tests. Bun 1.2.19 or later is required because earlier versions omit test files from their JUnit reports; `test.concurrent` needs Bun 1.3 or later. Generated tests deliberately fail until the expected application behavior is implemented. `scan` reports possible propagation and unresolved calls; it never claims to find every runtime exception.

Use `errspec --help` for commands and options. `verify` uses all runner-discovered tests by default; `--only-contract-tests` selects the registered files for gradual adoption. A reviewed baseline plus `--strict-baseline` detects removal of required checks from both the configuration and tests.

`errspec mutate` independently checks existing Vitest tests against 18 mutation families. This command requires Node.js 24+ and a local Vitest 5.x installation, and needs neither errspec configuration nor testkit. The native binary embeds its driver; users do not build JavaScript assets. Use `--vitest-project NAME` to select one Node project, `--format json` for the `errspec/mutation` report, and `--fail-on-undetected` to exit 1 on survived or uncovered mutations. Incomplete runs exit 2. Sources are transformed in memory and snapshot updates are disabled. Browser Mode, native require, and separate child processes are outside its instrumentation scope.

## Bundled Agent Skills

The CLI package includes `skills/errspec`, `skills/errspec-test-loop` and
`skills/errspec-playwright` in the skills-npm layout. They cover installation and
runner choice, behavior-focused gap repair, and the Playwright fixture/Vite setup.
The same files are embedded in the native executable:

```sh
errspec init-skill
errspec init-skill --dir .claude/skills
```

The default destination is `.agents/skills`. Identical copies remain unchanged;
a differing file stops the whole preflight before any other skill is written.
`--force` replaces only bundled files and retains unrelated project notes. Symlinked
targets/directories are refused. A later write failure rolls back this operation's
new files and overwritten bytes when they have not changed concurrently.
`--format json` reports written/unchanged skills or `EC_SKILL_INIT` with exit 2.
This command installs instructions; it does not install dependencies or run tests.
