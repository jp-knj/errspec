# Valibot adoption experiment

This experiment uses Valibot's real `StringIssue` and `safeParse(string(), input)` implementation at commit [`d72f76fcf70ccfa0a5ca341b80da1d440ac60b11`](https://github.com/open-circle/valibot/tree/d72f76fcf70ccfa0a5ca341b80da1d440ac60b11). It does not substitute a toy implementation or modify upstream source.

```sh
git clone --depth 1 https://github.com/open-circle/valibot.git ../valibot
git -C ../valibot fetch --depth 1 origin d72f76fcf70ccfa0a5ca341b80da1d440ac60b11
cargo build --release --locked
node scripts/pilot-valibot.mjs ../valibot target/release/errspec
```

Node.js, npm, Bun, Git and tar must be available. The script archives the pinned `library/` source into a temporary directory, installs the local testkit offline and removes the temporary directory afterward. Results are written to `artifacts/valibot-pilot.json`. The checkout supplied to the command is read-only.

The managed type is `StringIssue` in `library/src/schemas/string/string.ts`, using `type` as its discriminant. One scenario supplies the number `123`, requires rejection as a string issue and verifies that the issue preserves that input.

| Change | Expected result |
| --- | --- |
| Generate the initial test | Fails until application behavior is supplied |
| Complete the scenario | Verification passes |
| Remove its registration | EC001 |
| Remove the input comparison only | Bun passes; errspec reports EC010 |
| Remove both the comparison and its requirement | Default verification passes; strict baseline reports EC014 |
| Restore the scenario | Strict verification passes |

The experiment adds six setup files including package manifests and a baseline, with zero upstream source edits. The release-build run found 1,080 tagged-type candidates across 572 source files, with 527 unresolved call/use sites. Many candidates are schemas or re-exports, not distinct error types. This is a concrete limit of automatic discovery: candidate review and explicit type selection remain necessary, and an unresolved call is not evidence that it cannot fail.

The script records individual command durations and tool versions. These are execution times on one machine, not measured human onboarding time, comparative performance benchmarks, or evidence of long-term adoption. It verifies one schema and one scenario; it does not establish coverage of all Valibot failures.

## Try the packaged CLI

The same experiment can install the CLI, native binary and testkit from tarballs instead of using the development binary and local testkit directory. Prepare packages on the matching host, then run:

```sh
node scripts/pack.mjs linux-x64 target/release/errspec
node scripts/pilot-valibot.mjs ../valibot --packages artifacts/npm/manifest-linux-x64.json
```

Use `darwin-arm64` or `darwin-x64` for the corresponding macOS host. Package hashes are checked before installation. The installation uses a fresh npm cache, offline mode and disabled lifecycle scripts; every errspec command runs through the installed npm launcher. Results are written to `artifacts/valibot-packaged-pilot.json`.

The [recorded packaged run](packaged-result.json) on Linux x64 used errspec commit `08b184ed7baa27a329abe6e3e952b493413a78e1` and passed all seven checks. The runtime source was unchanged for this experiment. Package creation requires the maintainer's Rust toolchain; installation and use of the prepared tarballs do not. This local binary was not checked for compatibility with older glibc versions; use Distribution workflow artifacts for testers on the documented supported hosts.

## Starting with the upstream tsconfig

A separate [first-use probe](first-use-result.json) tried `init` with the unmodified `library/tsconfig.json`. Only errspec's three packages were installed in this archived source copy, so missing Vitest types are an environment limitation, not evidence that an installed Valibot project is incompatible.

| Configuration | Observed result |
| --- | --- |
| Unmodified upstream config | Exit 2, no generated files; TS5108 for `moduleResolution=node10` and missing test dependencies among 563 diagnostic lines |
| Original compiler options, with `include` replaced by `files: ["library/src/schemas/string/string.ts"]` in a root-level probe config | Exit 2; only TS5108 remains |
| Same focused config, changing only `moduleResolution` to `bundler` | Exit 0; generates the `string` case |

The middle probe isolates a configuration incompatibility with the pinned tsrs version from missing test dependencies. The successful adoption script already uses a focused Bundler config, so its green result must not be read as successful onboarding with the upstream config unchanged. A separate config changes analysis scope and module resolution; this result is not a recommendation to rewrite the application's production tsconfig.

## Findings to take into a user trial

The automated trial establishes that deleting a required comparison can leave an ordinary Bun test passing while errspec fails, and that a strict baseline catches deleting both the comparison and its requirement. It does not establish whether developers find that protection worth maintaining.

Observed setup costs and questions to test:

| Observation | Question for a developer using their own project |
| --- | --- |
| The upstream config fails; a focused config with different module resolution succeeds. | Can you identify the configuration problem and choose a valid analysis configuration without assistance? |
| The experiment adds six setup files, including manifests and a focused tsconfig. | Which additions fit an existing project, and which feel like duplicate maintenance? |
| Scan returns 1,080 candidates for 572 files, including schemas and re-exports. | Can you choose the intended error type without help, or would you start with a known export? |
| The pure validation example wraps a constant input in an asynchronous `s.fault` function. | Does the fault model fit input validation, or feel natural only around dependency failures? |
| Removing a comparison and its requirement passes default verification; strict baseline detects it. | Is the distinction understandable, and is baseline review practical in your PR workflow? |

These are observations and hypotheses from an agent-run trial. No human participants or Valibot maintainer feedback have been collected. Use the [first-use trial guide](feedback.md) to collect independent feedback before choosing the next feature.
