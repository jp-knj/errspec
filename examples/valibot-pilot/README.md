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

The experiment adds six setup files including package manifests and a baseline, with zero upstream source edits. The initial debug-build run found 1,080 tagged-type candidates across 572 source files, with 518 unresolved call/use sites. Many candidates are schemas or re-exports, not distinct error types. This is a concrete limit of automatic discovery: candidate review and explicit type selection remain necessary, and an unresolved call is not evidence that it cannot fail.

The script records individual command durations and tool versions. These are execution times on one machine, not measured human onboarding time, comparative performance benchmarks, or evidence of long-term adoption. It verifies one schema and one scenario; it does not establish coverage of all Valibot failures.
