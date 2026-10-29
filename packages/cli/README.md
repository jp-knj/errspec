# @errspec/cli

Native Rust CLI for discovering TypeScript error candidates and verifying explicitly registered failure scenarios with Bun or Vitest.

This package is being prepared for its first release. Build artifacts can be installed from local tarballs; registry installation is available only after publication.

For local tarballs, install the CLI, testkit and matching native package together. For example, on Linux x64: `npm install -D ./errspec-cli-0.1.0.tgz ./errspec-testkit-0.1.0.tgz ./errspec-cli-linux-x64-0.1.0.tgz`.

The npm launcher selects a matching native optional dependency. It performs no downloads or Rust builds in install scripts. Node.js 22 or later is required. Build targets: Linux x64 (glibc 2.35+), macOS arm64 and macOS x64. Windows and Linux musl are not included.

```sh
errspec scan --project tsconfig.json --format json
errspec init --project tsconfig.json --source src/errors.ts --export Failure --runner bun
errspec verify --runner bun --only-contract-tests
```

Install `@errspec/testkit` at the same version and your chosen runner before executing the generated tests. Generated tests deliberately fail until the expected application behavior is implemented. `scan` reports possible propagation and unresolved calls; it never claims to find every runtime exception.

Use `errspec --help` for commands and options. `verify` uses all runner-discovered tests by default; `--only-contract-tests` selects the registered files for gradual adoption. A reviewed baseline plus `--strict-baseline` detects removal of required checks from both the configuration and tests.
