# Mutant compiler facts

`@errspec/typecheck` independently acquires compiler facts from the project's own TypeScript 5/6 compiler API or TypeScript 7 synchronous native API. The embedded mutation driver imports this same implementation. It compares diagnostics against the original program and can identify narrow optional-chain equivalence under strict nullability and honest indexed-access settings. It never executes tests or assigns runtime mutation verdicts.

```js
import {readFileSync} from "node:fs";
import {checkMutants} from "@errspec/typecheck";
const source = readFileSync("src/value.ts", "utf8");
const original = "42";
const offset = source.indexOf(original);
const facts = await checkMutants({root: process.cwd()}, [{
  key: "number-contract",
  file: "src/value.ts",
  byteOffset: Buffer.byteLength(source.slice(0, offset)),
  end: Buffer.byteLength(source.slice(0, offset + original.length)),
  original,
  replacement: "'invalid number'",
}]);
console.log(facts.outcomes);
```

Use `project` for an explicit tsconfig and `typescriptPackage` for a specific installed compiler package. Each call creates and closes its compiler context. Inputs contain the original text and UTF-8 byte range; mismatches and duplicate keys are rejected. Source files stay unchanged. Each result contains `diagnostic` and `equivalentReason` facts, with `null` when absent; the native mutation pipeline makes the corresponding mutation classification.

Optional-chain equivalence requires an exact edit to one optional-chain token,
leaving the rest of the access or call unchanged. Property access retains its dot;
indexed access and calls remove the token. A family label does not suppress
diagnostics for a changed property, index, argument, suffix, call form, or second
optional token. Removing either single token from a longer chain is considered
independently under the existing receiver-type conditions.

`checkMutantsInWorker(options, mutants, {signal})` acquires the same facts on a Node worker thread. It supports cancellation and keeps synchronous compiler work off the calling thread. `noCheck` is rejected. Existing original diagnostics are excluded by message, file, code and multiplicity, rather than by shifting line positions.

This package has no CLI or native errspec dependency. Install its staged local tarball and use a project compiler; npm/crates.io publication remains a separate release operation.
