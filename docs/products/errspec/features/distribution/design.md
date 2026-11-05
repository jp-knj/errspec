# npm distribution and initial generation design

This design implements the [requirements](requirements.md) and [Product](../../product.md). It is for maintainers of the CLI and distribution. Developers still choose the meaning of an error and its expected business behavior.

## Launcher and artifacts

A small Node launcher resolves the matching OS/CPU optional dependency and starts its native binary. There is no install/postinstall download or compilation. The prebuilt targets are Linux x64 glibc and macOS arm64/x64. Each target's CI builds, packs, installs, and executes artifacts; current evidence and pending checks are tracked in [implementation status](../../implementation-status.md).

The launcher and native packages share a version. Packaging includes required source, licenses, launchers, declarations, and binaries, excluding tests and temporary execution records. The [distribution guide](../../../../engineering/guidelines/distribution.md) describes artifact validation and the larger package set.

## Initial generation

Generation accepts an internal request independent of argument-parser types. It analyzes the exported type through the shared analysis boundary, obtains every finite tag, and builds a configuration/test pair. The generated configuration uses schemaVersion 2.

I/O resolves paths and acquires the tags and configuration-relative paths. A pure assembly step builds one case-ID/tag list and derives both files from it. It accesses neither the compiler nor the filesystem. This keeps configuration and test identities aligned without duplicating decisions.

Both contents are complete before writing. Destination preflight precedes type analysis and writes, preserving existing validation order, JSON key order, and string escaping. `create_new` also rejects a race after preflight. On failure, cleanup removes only files newly created by that invocation. Existing files are never replaced.

The result is a typed list of files, tags, and next steps, rendered through existing text/JSON output. Analysis uses the common one-shot analysis thread boundary.

## Interface

```sh
errspec init --project tsconfig.json --source src/errors.ts --export Failure --discriminant _tag --runner bun
```

The defaults are `errspec.json` and `errspec.generated.test.ts`. `--test` selects the test destination, `--contract` the contract ID, and the existing `--config` the configuration destination. Generated tests throw a TODO error until developers provide expectations, so a scaffold cannot pass accidentally. Embedded strings use JSON escaping.

## Publication and security

npm performs ordinary integrity-checked package resolution. GitHub Actions produces native binaries and npm tarball artifacts. Packing requires no publication rights; credentials are not embedded in source. Before publication, locally install tarballs and identify them as unpublished artifacts.

A postinstall downloader was rejected because it adds another network path and requires install scripts. Platform-specific packages use npm's existing integrity checks. Linux musl, Windows, or other targets are added only after actual build and execution validation. Scope ownership and first publication authentication remain release-time work.
