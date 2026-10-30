// Vitest setup file injected by `errspec coverage`. It never changes assertion results.
// Each passing positive assertion records the discriminant values in the value it received,
// so existing tests count as evidence without errspec-specific test code.
import { appendFileSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { threadId } from "node:worker_threads";

const MATCHERS = [
  "toBe",
  "toEqual",
  "toStrictEqual",
  "toMatchObject",
  "toContain",
  "toContainEqual",
  "toHaveProperty",
];
// Bounds keep recording cheap for large received values such as DOM trees.
const MAX_DEPTH = 6;
const MAX_NODES = 2000;
const MAX_STRING = 200;

const directory = process.env.ERRSPEC_COVERAGE;
const vitestPackage = process.env.ERRSPEC_VITEST_PACKAGE;
const keys = new Set((process.env.ERRSPEC_DISCRIMINANTS ?? "").split(",").filter(Boolean));

// Import the same Vitest instance the tests use; this file lives outside the project.
function entry(packageDirectory) {
  const manifest = JSON.parse(readFileSync(join(packageDirectory, "package.json"), "utf8"));
  let target = manifest.exports?.["."] ?? manifest.exports;
  while (target && typeof target === "object") target = target.import ?? target.default;
  return join(packageDirectory, typeof target === "string" ? target : (manifest.module ?? manifest.main));
}

/** Discriminant [key, value] pairs, plus bare strings as ["", value]. */
export function collect(received, discriminants) {
  const found = new Map();
  const seen = new Set();
  let nodes = 0;
  const add = (key, value) => {
    if (value.length <= MAX_STRING) found.set(`${key}\u0000${value}`, [key, value]);
  };
  const visit = (value, depth) => {
    if (typeof value === "string") return add("", value);
    if (!value || typeof value !== "object" || depth > MAX_DEPTH || seen.has(value)) return;
    if (++nodes > MAX_NODES) return;
    seen.add(value);
    for (const [key, child] of Object.entries(value)) {
      if (typeof child !== "string") visit(child, depth + 1);
      else if (discriminants.has(key)) add(key, child);
      // Free-text properties such as messages are not tags; array items may be.
      else if (Array.isArray(value)) add("", child);
    }
  };
  visit(received, 0);
  return [...found.values()];
}

if (directory && vitestPackage) {
  const { chai, expect } = await import(pathToFileURL(entry(vitestPackage)).href);
  const output = join(directory, `${process.pid}-${threadId}.jsonl`);
  chai.use((api, utils) => {
    for (const name of MATCHERS) {
      if (typeof api.Assertion.prototype[name] !== "function") continue;
      api.Assertion.overwriteMethod(name, original => function (...args) {
        const result = original.apply(this, args);
        // A throw above means the assertion failed; negated assertions prove absence, not presence.
        if (!utils.flag(this, "negate")) {
          const values = collect(utils.flag(this, "object"), keys);
          if (values.length) {
            const { testPath, currentTestName } = expect.getState();
            appendFileSync(output, `${JSON.stringify({ file: testPath, test: currentTestName, matcher: name, values })}\n`);
          }
        }
        return result;
      });
    }
  });
}
