// End-to-end acceptance check against real Vitest. Only mutates a temporary copy.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const example = join(project, "examples/checkout");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
assert(existsSync(binary), "Run cargo build first");
assert(existsSync(join(example, "node_modules/.bin/vitest")), "Run npm ci in examples/checkout first");
const work = mkdtempSync(join(tmpdir(), "errspec-demo-"));
const results = [];

function run(label, args, status) {
  const result = spawnSync(binary, [...args, "--format", "json"], { cwd: work, encoding: "utf8", timeout: 60_000 });
  assert.equal(result.error, undefined, String(result.error));
  assert.equal(result.status, status, `${label}: ${result.stderr}\n${result.stdout}`);
  const report = JSON.parse(result.stdout);
  results.push({ label, exitCode: result.status, report });
  console.log(`${label}: exit ${result.status}`);
  return report;
}

try {
  cpSync(example, work, { recursive: true, filter: (p) => !p.includes("node_modules") });
  symlinkSync(join(example, "node_modules"), join(work, "node_modules"), process.platform === "win32" ? "junction" : "dir");
  const missing = run("missing variant", ["check", "--baseline", "baseline.json"], 1);
  assert(missing.diagnostics.some((d) => d.code === "EC001" && d.tag === "RateLimited"));
  assert.deepEqual(missing.delta.added["checkout.payment"], ["RateLimited"]);
  const registered = run("registration without test execution", ["check", "--config", "errspec.complete.json"], 0);
  assert.equal(registered.testsExecuted, false);
  const green = run("real Vitest green", ["verify", "--config", "errspec.complete.json"], 0);
  assert(green.contracts[0].cases.every((c) => c.testStatus === "passed"));
  const source = join(work, "src/checkout.ts");
  const original = readFileSync(source, "utf8");
  assert(original.includes('case "RateLimited": return { status: 429 };'));
  writeFileSync(source, original.replace('case "RateLimited": return { status: 429 };', 'case "RateLimited": return { status: 200 };'));
  const broken = run("failure converted to success is rejected", ["verify", "--config", "errspec.complete.json"], 1);
  assert(broken.diagnostics.some((d) => d.code === "EC005" && d.tag === "RateLimited"));
  writeFileSync(source, original);
  const testFile = join(work, "tests/checkout.test.ts");
  const test = readFileSync(testFile, "utf8");
  writeFileSync(testFile, test.replace('it("RateLimited', 'it.skip("RateLimited'));
  const skipped = run("skipped case is rejected", ["verify", "--config", "errspec.complete.json"], 1);
  assert(skipped.diagnostics.some((d) => d.code === "EC005" && d.tag === "RateLimited"));
  // Normalise environment-specific strings before retaining an example report.
  const evidence = JSON.stringify({ acceptance: "passed", scenarios: results }, null, 2)
    .replaceAll(work, "<temporary-project>");
  mkdirSync(join(project, "artifacts"), { recursive: true });
  writeFileSync(join(project, "artifacts/acceptance.json"), evidence + "\n");
  console.log("All 5 acceptance scenarios passed");
} finally {
  rmSync(work, { recursive: true, force: true });
}
