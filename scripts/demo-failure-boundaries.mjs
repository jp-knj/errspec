// Real Vitest checks for failure forwarding and required observations.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
const deps = join(project, "examples/checkout/node_modules");
assert(existsSync(binary), "Run cargo build first");
assert(existsSync(join(deps, ".bin/vitest")), "Run npm ci in examples/checkout first");
const work = mkdtempSync(join(tmpdir(), "errspec-failure-boundaries-"));
const source = String.raw`
export const FAILURE_CODES = { offline: "Offline", corrupt: "Corrupt" } as const;
export type FailureCode = typeof FAILURE_CODES[keyof typeof FAILURE_CODES];
export interface ReadFailure { readonly kind: FailureCode; readonly detail: string }
export interface UnusedFailure { readonly kind: "Offline"; readonly other: number }
export type ReadResult = { readonly ok: true; readonly value: string } | { readonly ok: false; readonly error: ReadFailure };
export async function forward(deps: { read: () => Promise<ReadResult>; write: (value: string) => Promise<void> }): Promise<ReadResult> {
  const result = await deps.read();
  if (!result.ok) return result;
  await deps.write(result.value);
  return result;
}
`;
const scenario = String.raw`
import { test } from "vitest";
import { scenarioTest } from "TESTKIT_ENTRY";
import { FAILURE_CODES, forward } from "./source.ts";
for (const [id, kind] of Object.entries(FAILURE_CODES)) {
  scenarioTest(test, import.meta.url, { contract: "read", id, tag: kind }, async s => {
    let writes = 0;
    const failure = { ok: false as const, error: { kind, detail: "injected" } };
    const result = await forward({
      read: s.fault("read", async () => failure),
      write: async () => { writes++; },
    });
    s.check("result", result, failure);
    s.check("writes", writes, 0);
  });
}
`.replace("TESTKIT_ENTRY", pathToFileURL(join(project, "packages/testkit/index.mjs")).href);
const plain = String.raw`
import { expect, it } from "vitest";
import { FAILURE_CODES, forward } from "./source.ts";
for (const [id, kind] of Object.entries(FAILURE_CODES)) it(id, async () => {
  const failure = { ok: false as const, error: { kind, detail: "injected" } };
  const result = await forward({ read: async () => failure, write: async () => {} });
  expect(result).toEqual(failure);
});
`;
function run(label, args, status) {
  const p = spawnSync(binary, [args[0], "--format", "json", ...args.slice(1)], { cwd: work, encoding: "utf8", timeout: 60000 });
  assert.equal(p.error, undefined, String(p.error));
  assert.equal(p.status, status, label + ": " + p.stderr + "\n" + p.stdout);
  console.log(label + ": exit " + p.status);
  return JSON.parse(p.stdout);
}
function allTestsPassed(report) {
  assert.equal(report.testsExecuted, true);
  assert(report.contracts.every(c => c.cases.every(x => x.testStatus === "passed")));
}
try {
  symlinkSync(deps, join(work, "node_modules"), process.platform === "win32" ? "junction" : "dir");
  writeFileSync(join(work, "package.json"), JSON.stringify({ type: "module", private: true }));
  writeFileSync(join(work, "tsconfig.json"), JSON.stringify({ compilerOptions: { target: "ES2022", module: "ESNext", moduleResolution: "Bundler", strict: true, noEmit: true, types: [] }, files: ["source.ts"] }));
  writeFileSync(join(work, "vitest.config.mjs"), "export default {cacheDir:'.cache',test:{include:['*.test.ts'],cache:false,fileParallelism:false,maxWorkers:1,watch:false,update:false}};\n");
  writeFileSync(join(work, "source.ts"), source);
  writeFileSync(join(work, "plain.test.ts"), plain);
  writeFileSync(join(work, "scenario.test.ts"), scenario);
  writeFileSync(join(work, "errspec.json"), JSON.stringify({ schemaVersion: 2, project: "tsconfig.json", contracts: [{ id: "read", source: { file: "source.ts", export: "ReadFailure", discriminant: "kind" }, cases: Object.entries({ offline: "Offline", corrupt: "Corrupt" }).map(([id, tag]) => ({ id, tag, expect: "Return the completed read failure before attempting a write", test: { file: "scenario.test.ts" }, observe: { faults: ["read"], assertions: ["result", "writes"] } })) }] }));
  const coverage = run("existing passing comparisons", ["coverage", "--", "plain.test.ts"], 0);
  assert.equal(coverage.runnerOk, true);
  assert.deepEqual(coverage.types.find(t => t.source.export === "ReadFailure").tags.map(t => [t.tag, t.status]), [["Corrupt", "asserted"], ["Offline", "asserted"]]);
  // This is tag evidence across the suite, not execution evidence for each declaration.
  assert.equal(coverage.types.find(t => t.source.export === "UnusedFailure").tags[0].status, "asserted");
  const verify = ["verify", "--only-contract-tests"];
  const green = run("read failure stops before writing", verify, 0);
  allTestsPassed(green);
  assert(green.contracts.every(c => c.cases.every(x => x.observationStatus === "passed")));

  writeFileSync(join(work, "scenario.test.ts"), scenario.replace('    s.check("result", result, failure);\n', ""));
  const unchecked = run("passing tests without required comparison", verify, 1);
  allTestsPassed(unchecked);
  assert.equal(unchecked.diagnostics.filter(d => d.code === "EC010").length, 2);

  writeFileSync(join(work, "scenario.test.ts"), scenario.replace('s.fault("read", async () => failure)', 's.fault("other", async () => failure)'));
  const direct = run("passing tests with a different observed dependency", verify, 1);
  allTestsPassed(direct);
  assert.equal(direct.diagnostics.filter(d => d.code === "EC009").length, 2);

  writeFileSync(join(work, "scenario.test.ts"), scenario);
  writeFileSync(join(work, "source.ts"), source.replace("if (!result.ok) return result;", 'if (!result.ok) return { ok: true, value: "fallback" };'));
  const converted = run("failure converted to success", verify, 1);
  assert.equal(converted.contracts[0].cases.filter(c => c.testStatus === "failed").length, 2);

  writeFileSync(join(work, "source.ts"), source.replace("if (!result.ok) return result;", 'if (!result.ok) { await deps.write("unexpected"); return result; }'));
  const wrote = run("write attempted after failed read", verify, 1);
  assert.equal(wrote.contracts[0].cases.filter(c => c.testStatus === "failed").length, 2);
  assert.equal(wrote.diagnostics.filter(d => d.code === "EC010" && d.message.includes("writes")).length, 2);
  console.log("All 6 failure-boundary acceptance checks passed");
} finally {
  rmSync(work, { recursive: true, force: true });
}
