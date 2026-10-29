# Preparing npm packages

The registry packages are not published yet. Keep CLI, testkit, native optional dependencies and Cargo versions in sync.

Build and pack on the matching host:

```sh
cargo build --release --locked
node scripts/pack.mjs linux-x64 target/release/errspec
node scripts/package-smoke.mjs artifacts/npm/manifest-linux-x64.json
```

Use `darwin-arm64` or `darwin-x64` on the corresponding macOS host. The Distribution workflow builds all three targets, runs the offline consumer smoke test and uploads tarballs plus a SHA-256 manifest as artifacts. Linux releases are built on Ubuntu 22.04 for glibc 2.35 compatibility. Windows and musl are not included.

To try the unpublished packages, download one platform's artifact and install all three tarballs together:

```sh
npm install -D ./errspec-cli-0.1.0.tgz ./errspec-testkit-0.1.0.tgz ./errspec-cli-linux-x64-0.1.0.tgz
npx errspec scan --project tsconfig.json --format json
```

Use the matching native tarball on macOS. No install scripts are required. The smoke test uses an empty npm cache and offline installation, then checks the real CLI, generated TODO failure, completed scenario, selected-file execution and full-suite failure.

Native packages contain dependency license texts collected from the Cargo resolution for that platform. Packaging fails if a dependency's license text cannot be located. Rust 1.99.1 thin LTO failed to link during the first release build; normal release optimization is used and the actual binaries are tested.

Publication is a separate operation once npm scope ownership and authentication are configured. Publish the three native packages and testkit before the CLI. This repository does not automatically publish packages, create public releases or change repository visibility when a PR is opened or merged.
