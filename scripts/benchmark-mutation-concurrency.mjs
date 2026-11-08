// Public 12-file/60-mutant fixture. Measure the complete native process tree.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, realpathSync, symlinkSync, writeFileSync } from "node:fs";
import { arch, cpus, platform, release, totalmem } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
  binary: { type: "string" }, output: { type: "string" },
  label: { type: "string", default: "before" },
  dependencies: { type: "string", default: "examples/checkout/node_modules" },
  repeats: { type: "string", default: "3" },
} });
assert(values.binary && values.output, "Use --binary BINARY --output DIRECTORY [--label before|after] [--repeats 3]");
assert(["before", "after"].includes(values.label), "label must be before or after");
const repeats = Number(values.repeats);
assert(Number.isSafeInteger(repeats) && repeats > 0, "repeats must be positive");
const output = resolve(values.output), binary = resolve(values.binary);
mkdirSync(output, { recursive: true });
const destination = join(output, values.label);
mkdirSync(destination); // Never replace earlier observations.
const root = join(output, "fixture");
if (values.label === "before") {
  mkdirSync(root); mkdirSync(join(root, "src"));
  symlinkSync(realpathSync(values.dependencies), join(root, "node_modules"), "dir");
  writeFileSync(join(root, "package.json"), JSON.stringify({ private: true, type: "module", dependencies: { vitest: "5.0.5" } }));
  writeFileSync(join(root, "vitest.config.mjs"), 'export default {test:{include:["*.test.ts"]}};\n');
  for (let index = 0; index < 12; index++) {
    writeFileSync(join(root, `src/value-${index}.ts`), `export function value(n:number) { return n >= ${index + 1}; }\n`);
    writeFileSync(join(root, `value-${index}.test.ts`), `import {test,expect} from "vitest";import {value} from "./src/value-${index}.ts";test("boundary",()=>{expect(value(${index + 1})).toBe(true);expect(value(${index})).toBe(false)});\n`);
  }
}
const digest = value => createHash("sha256").update(value).digest("hex");
const inputs = Object.fromEntries([
  ...readdirSync(root).filter(name => /\.(json|mjs|ts)$/.test(name)),
  ...readdirSync(join(root, "src")).map(name => `src/${name}`),
].sort().map(name => [name, digest(readFileSync(join(root, name)))]));
const metadata = {
  label: values.label, binary, binarySha256: digest(readFileSync(binary)),
  node: process.version, platform: platform(), arch: arch(), release: release(),
  cpu: cpus()[0].model, logicalCpus: cpus().length, memoryBytes: totalmem(),
  fixture: root, inputs, dependencies: realpathSync(join(root, "node_modules")),
  vitest: JSON.parse(readFileSync(join(root, "node_modules/vitest/package.json"), "utf8")).version,
  sampleIntervalMs: 100, repeats, concurrency: [1, 2, 4],
  mode: "fresh sessions, result/probe caches disabled, default candidate selection",
};
writeFileSync(join(destination, "metadata.json"), JSON.stringify(metadata, null, 2) + "\n");
let oracle;
if (values.label === "after") {
  const previous = JSON.parse(readFileSync(join(output, "before/metadata.json"), "utf8"));
  assert.deepEqual(metadata.inputs, previous.inputs);
  for (const key of ["node", "dependencies", "vitest", "mode"]) assert.equal(metadata[key], previous[key]);
  oracle = JSON.parse(readFileSync(join(output, "before/semantic.json"), "utf8"));
}
function semantics(report) {
  // Durations, worker-count metadata and internal task IDs vary per execution.
  return {
    runComplete: report.runComplete, ok: report.ok, testsExecuted: report.testsExecuted,
    summary: report.summary, gate: report.gate, diagnostics: report.diagnostics,
    baseline: report.baseline.map(({ id, file, name, status, started }) => ({ id, file, name, status, started })),
    mutants: report.mutants,
  };
}
function sample(pid) {
  const result = spawnSync("ps", ["-axo", "pid=,ppid=,rss="], { encoding: "utf8" });
  assert.ifError(result.error); assert.equal(result.status, 0, result.stderr);
  const rows = result.stdout.trim().split("\n").map(row => row.trim().split(/\s+/).map(Number));
  const descendants = new Set([pid]);
  let count;
  do {
    count = descendants.size;
    for (const [id, parent] of rows) if (descendants.has(parent)) descendants.add(id);
  } while (count !== descendants.size);
  const owned = rows.filter(([id]) => descendants.has(id));
  return { rssKiB: owned.reduce((sum, [, , rss]) => sum + rss, 0), processes: owned.length };
}
async function measure(concurrency, repeat) {
  const args = ["mutate", "--root", root, "--node", process.execPath, "--format", "json", "--no-cache", "--no-reuse-session", "--concurrency", String(concurrency)];
  const samples = [], start = performance.now();
  const child = spawn(binary, args, { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "", stderr = "", samplingError;
  child.stdout.on("data", bytes => { stdout += bytes; });
  child.stderr.on("data", bytes => { stderr += bytes; });
  const timer = setInterval(() => {
    try { samples.push({ elapsedMs: performance.now() - start, ...sample(child.pid) }); }
    catch (error) { samplingError = error; child.kill("SIGINT"); }
  }, 100);
  let forcedStop;
  const deadline = setTimeout(() => {
    child.kill("SIGINT");
    forcedStop = setTimeout(() => child.kill("SIGKILL"), 5000);
  }, 180000);
  let code, signal;
  try {
    [code, signal] = await new Promise((accept, reject) => {
      child.once("error", reject); child.once("close", (code, signal) => accept([code, signal]));
    });
  } finally { clearInterval(timer); clearTimeout(deadline); clearTimeout(forcedStop); }
  const elapsedMs = performance.now() - start;
  const prefix = join(destination, `j${concurrency}-${repeat}`);
  writeFileSync(prefix + ".stdout.json", stdout);
  writeFileSync(prefix + ".stderr.txt", stderr);
  writeFileSync(prefix + ".samples.json", JSON.stringify(samples) + "\n");
  assert.ifError(samplingError); assert.equal(code, 0, stderr); assert.equal(signal, null);
  assert(samples.length > 0 && samples.some(item => item.processes > 1), "No descendant samples acquired");
  const report = JSON.parse(stdout), current = semantics(report);
  assert.equal(report.runComplete, true); assert.equal(report.baseline.length, 12);
  assert.equal(report.mutants.length, 60); assert(report.mutants.every(mutant => mutant.status === "Killed"));
  assert.equal(report.engine.parallel?.workers ?? 1, concurrency);
  oracle ??= current;
  assert.deepEqual(current, oracle, "Candidate order, verdicts, or evidence changed");
  writeFileSync(join(destination, "semantic.json"), JSON.stringify(oracle, null, 2) + "\n");
  return { concurrency, repeat, elapsedMs, peakTreeRssKiB: Math.max(...samples.map(item => item.rssKiB)),
    peakProcesses: Math.max(...samples.map(item => item.processes)), samples: samples.length,
    hostExit: code, mutants: report.mutants.length, killed: report.summary.killed };
}
const observations = [];
for (let repeat = 0; repeat < repeats; repeat++) {
  // Rotate order to distribute the first position across all worker counts.
  const order = [[1, 2, 4], [2, 4, 1], [4, 1, 2]][repeat % 3];
  for (const concurrency of order) {
    const result = await measure(concurrency, repeat);
    observations.push(result);
    writeFileSync(join(destination, "observations.json"), JSON.stringify(observations, null, 2) + "\n");
    console.log(JSON.stringify(result));
  }
}
