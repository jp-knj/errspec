# Preparing npm packages

Registry packages are not published. Keep Cargo, CLI, core, testkit, adapters, and native optional-dependency versions aligned as the package inventory grows. Publication and repository visibility/migration remain separate later work.

## Build and validate artifacts

Build and pack on the matching host. Install the pinned development dependencies
and Chromium used by the runner fixtures before the installed-runner smoke; the
[Distribution workflow](../../../.github/workflows/distribution.yml) defines the
setup and platform-specific prerequisites:

```sh
node scripts/build-testkit.mjs --check
cargo build --release --locked
node scripts/pack.mjs linux-x64 target/release/errspec
node scripts/package-smoke.mjs artifacts/npm/manifest-linux-x64.json
node scripts/report-package-smoke.mjs artifacts/npm/manifest-linux-x64.json
node scripts/action-package-smoke.mjs artifacts/npm/manifest-linux-x64.json
node scripts/skills-package-smoke.mjs artifacts/npm/manifest-linux-x64.json
node scripts/runner-package-smoke.mjs artifacts/npm/manifest-linux-x64.json
```

Use `darwin-arm64` or `darwin-x64` on the corresponding macOS host. [Distribution CI](../../../.github/workflows/distribution.yml) builds all three targets, validates installed artifacts, and uploads tarballs with a SHA-256 manifest. Linux uses Ubuntu 22.04 for glibc 2.35 compatibility. Windows and musl are unsupported. Required jobs must actually succeed; a billing-related failure before startup is not validation. Current evidence is in [implementation status](../../products/errspec/implementation-status.md).

Mutation-driver bundle changes require the pinned build/check described in the [driver guide](../../../packages/mutation-driver/README.md). Consumer installation requires no Rust compiler, lifecycle downloader, or JS build. Native packages include dependency license texts from the host's Cargo resolution; missing license text fails packaging. Rust 1.99.1 thin LTO failed to link during the initial release build, so normal release optimization is used and actual binaries are tested.

## Use unpublished tarballs

Download a single platform's artifact and install the matching package set from its manifest. For the CLI/testkit/core set:

```sh
npm install -D ./errspec-cli-0.1.0.tgz ./errspec-testkit-0.1.0.tgz ./errspec-core-0.1.0.tgz ./errspec-cli-linux-x64-0.1.0.tgz
npx errspec scan --project tsconfig.json --format json
```

Use the matching native tarball on macOS, and include adapter/typecheck tarballs when using those entry points. The manifest is authoritative for the delivered inventory; do not assume a fixed number of tarballs. This local-artifact example does not imply registry availability.

Installed smoke tests use fresh npm caches, integrity-checked local tarballs, and offline npm installation. They exercise real launcher/native discovery, generated TODO failure, completed scenarios, selected-file/full-suite behavior, core APIs, formatting, Action transport, and native-embedded Skills. Runner-package acceptance also checks complete matrices and Git test selection, real Chromium fixtures, strict declarations, and TS6/TS7 direct/worker facts including the optional-chain edit guard. The typecheck package is installed independently without the CLI. Successful packing alone does not establish these behaviors.

Mutation/ownership/package suites share a per-UID worker registry. Run ownership/fake-orphan tests and real runner suites in coordinated windows so cleanup fixtures cannot interfere. Formatter-only and skill-copy checks do not create mutation workers.

## Release boundary

Packing and artifact upload require no npm publication rights. Scope ownership, authentication, package publish order, and registry checks are release-time work after explicit publication authorization. Opening/merging a PR does not publish packages, create public releases, change visibility, or migrate the repository.
