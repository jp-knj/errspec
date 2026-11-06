# Distribution and initial verification generation

[Product](../../product.md) requires developers to verify a first failure case in an existing project without installing Rust.

## Scope

Provide npm packages, prebuilt binaries for supported platforms, configuration and test generation for an existing error type, and installation trials in an independent real project. Expected business behavior remains the developer's responsibility. Generation must not overwrite existing files. Creating public accounts or changing repository visibility is outside this work.

## User stories and acceptance

### Start with one existing type

Developers select one exported type, generate minimal configuration and tests, and supply the actual expected behavior.

- A type with finite failure tags produces a case and test for every tag. Unfinished expectations fail in an ordinary test run.
- If either output already exists, generation reports the conflicting path, preserves existing bytes, and leaves neither new output behind.

### Use the CLI without Rust

Developers install the npm distribution and execute its native CLI.

- On a supported platform, the launcher forwards arguments, standard streams, and exit status to the prebuilt native binary. It does not compile Rust on the user's machine.
- An unsupported platform or missing binary produces a nonzero exit with a useful installation or platform explanation. The launcher must not execute a binary for another platform.

## Functional requirements

Inputs are a tsconfig, exported type, discriminator, runner, and output paths. Outputs are configuration, tests, and next steps. CLI, testkit, and platform binaries use matching versions; the wider package surface is described in the [workflow requirements](../mutation-workflows/requirements.md).

Generation does not execute tests or install dependencies. Unresolvable types, ambiguous candidates, and output conflicts fail before writing. Supported OS/CPU combinations and unpublished status are explicit. Acceptance uses built, packed, locally installed artifacts.

## Evidence and data

An independent project trial records the fixed project revision, commands, number of changed files, detection of an unregistered tag, and detection of a removed mandatory assertion. Installation time or savings are not claimed before measurement.

Generation uses type/tag/runner/path data locally. Distribution metadata includes version, OS/CPU, binary, license, and checksum. Matching versions connect the launcher, native package, and testkit.

## External services and release boundary

npm and GitHub Actions support distribution preparation. Local tarballs and CI artifacts can be prepared without publishing credentials. Scope ownership and first publication authentication are checked when publication is authorized. Unpublished artifacts must not be described as already available from the public registry.

No install script downloads executables from arbitrary URLs. Existing source and tests are not automatically rewritten. Additional platforms require actual build and execution evidence before entering the supported list.
