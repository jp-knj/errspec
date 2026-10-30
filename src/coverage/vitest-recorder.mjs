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
function entry(packageDirectory, subpath = ".") {
  const manifest = JSON.parse(readFileSync(join(packageDirectory, "package.json"), "utf8"));
  let target = manifest.exports?.[subpath] ?? (subpath === "." ? manifest.exports : undefined);
  while (target && typeof target === "object") target = target.import ?? target.default;
  return join(packageDirectory, typeof target === "string" ? target : (manifest.module ?? manifest.main));
}

// getState().currentTestName uses " > " on Vitest 4, while its JSON reporter
// joins task names with spaces. Preserve literal separators inside user titles.
export function testFullName(task, fallback) {
  if (!task) return fallback;
  const names = [];
  for (let current = task; current && current !== task.file; current = current.suite) {
    names.unshift(current.name);
  }
  return names.join(" ");
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
    // Matcher internals (e.g. objectContaining.sample) are not literal expectations.
    for (let prototype = value; prototype; prototype = Object.getPrototypeOf(prototype)) {
      if (Object.hasOwn(prototype, "asymmetricMatch")) return;
    }
    if (++nodes > MAX_NODES) return;
    seen.add(value);
    for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(value))) {
      // Recording must not invoke user getters a second time.
      if (!descriptor.enumerable || !("value" in descriptor)) continue;
      const child = descriptor.value;
      if (typeof child !== "string") visit(child, depth + 1);
      else if (discriminants.has(key)) add(key, child);
      // Free-text properties such as messages are not tags; array items may be.
      else if (Array.isArray(value)) add("", child);
    }
  };
  visit(received, 0);
  return [...found.values()];
}

// Only literal constraints imposed by the matcher are evidence. Receiving an object
// carrying a tag does not mean that a partial matcher checked that tag.
export function checkedValues(matcher, received, args, discriminants) {
  const expected = args[0];
  if (matcher === "toBe") {
    return typeof received === "string" && received === expected ? [["", received]] : [];
  }
  if (matcher === "toContain") {
    return Array.isArray(received) && typeof expected === "string" && received.includes(expected)
      ? [["", expected]] : [];
  }
  if (matcher === "toHaveProperty") {
    if (args.length < 2) return [];
    const path = args[0];
    const key = Array.isArray(path) ? path.at(-1)
      : typeof path === "string" && /^[\w$]+(?:\.[\w$]+|\[\d+\])*$/.test(path)
        ? path.split(/[.\[\]]/).filter(Boolean).at(-1) : undefined;
    return typeof args[1] === "string"
      ? discriminants.has(key) ? [[key, args[1]]] : []
      : collect(args[1], discriminants);
  }
  if (matcher === "toContainEqual") return collect(expected, discriminants);
  // A successful equality/partial-equality matcher guarantees these literal
  // constraints. Never collect extra properties from received.
  return collect(expected, discriminants);
}

if (directory && vitestPackage) {
  const { chai, expect, TestRunner } = await import(pathToFileURL(entry(vitestPackage)).href);
  let currentTest = TestRunner?.getCurrentTest;
  if (typeof currentTest !== "function") {
    // Vitest 3 exposes the same task API from its suite subpath.
    try { currentTest = (await import(pathToFileURL(entry(vitestPackage, "./suite")).href)).getCurrentTest; }
    catch { /* Older runners retain the conservative state-name fallback. */ }
  }
  const output = join(directory, `${process.pid}-${threadId}.jsonl`);
  chai.use((api, utils) => {
    for (const name of MATCHERS) {
      if (typeof api.Assertion.prototype[name] !== "function") continue;
      api.Assertion.overwriteMethod(name, original => function (...args) {
        const result = original.apply(this, args);
        // A throw above means the assertion failed; negated assertions prove absence, not presence.
        if (!utils.flag(this, "negate")) {
          let values;
          try { values = checkedValues(name, utils.flag(this, "object"), args, keys); }
          catch { return result; } // Uninspectable expectations are not evidence.
          if (values.length) {
            const { testPath, currentTestName } = expect.getState();
            const test = testFullName(currentTest?.(), currentTestName);
            appendFileSync(output, `${JSON.stringify({ file: testPath, test, matcher: name, values })}\n`);
          }
        }
        return result;
      });
    }
  });
}
