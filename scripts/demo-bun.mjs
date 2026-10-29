// Real Bun acceptance scenarios. Mutations are confined to a temporary copy.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
const explicitBun = process.argv[3] ? resolve(process.argv[3]) : undefined;
assert(existsSync(binary), "Run cargo build first");
const bunVersion = spawnSync(explicitBun ?? "bun", ["--version"], { encoding: "utf8" });
assert.equal(bunVersion.status, 0, "Install Bun or pass its executable as argument 3");
const work = mkdtempSync(join(tmpdir(), "errspec-bun-"));
const results = [];
const verify = ["verify", "--runner", "bun", "--config", "errspec.complete.json", ...(explicitBun ? ["--bun", explicitBun] : [])];

function run(label, args, status) {
  const result = spawnSync(binary, [...args, "--format", "json"], { cwd: work, encoding: "utf8", timeout: 60_000 });
  assert.equal(result.error, undefined, String(result.error));
  assert.equal(result.status, status, `${label}: ${result.stderr}\n${result.stdout}`);
  const report = JSON.parse(result.stdout);
  results.push({ label, exitCode: result.status, report });
  console.log(`${label}: exit ${result.status}`);
  return report;
}
function caseStatus(report, tag) { return report.contracts[0].cases.find(c => c.tag === tag).testStatus; }

try {
  cpSync(join(project, "examples/checkout-bun"), work, { recursive: true });
  const missing = run("unregistered variant", ["check"], 1);
  assert(missing.diagnostics.some(d => d.code === "EC001" && d.tag === "RateLimited"));
  const good = run("Bun green", verify, 0);
  assert.equal(good.testRunner, "bun");
  assert.equal(caseStatus(good, "RateLimited"), "passed");

  const source = join(work, "src/checkout.ts");
  const original = readFileSync(source, "utf8");
  writeFileSync(source, original.replace("status: 429", "status: 200"));
  assert.equal(caseStatus(run("wrong success response", verify, 1), "RateLimited"), "failed");
  writeFileSync(source, original);

  const file = join(work, "tests/checkout.test.ts");
  const originalTest = readFileSync(file, "utf8");
  writeFileSync(file, originalTest.replace('it("RateLimited', 'it.skip("RateLimited'));
  assert.equal(caseStatus(run("skip", verify, 1), "RateLimited"), "skipped");
  writeFileSync(file, originalTest.replace('it("RateLimited', 'it.todo("RateLimited'));
  assert.equal(caseStatus(run("todo", verify, 1), "RateLimited"), "todo");
  writeFileSync(file, originalTest.replace('it("RateLimited', 'it("Renamed RateLimited'));
  assert.equal(caseStatus(run("unmatched test name", verify, 1), "RateLimited"), "not_found");

  // Verify recursive describe order and XML entities, rather than classname string heuristics.
  writeFileSync(file, originalTest.replace('describe("checkout", () => {', 'describe("outer & <xml>", () => {\ndescribe("checkout", () => {') + '\n});\n');
  const configPath = join(work, "errspec.complete.json");
  const originalConfig = readFileSync(configPath, "utf8");
  const config = JSON.parse(originalConfig);
  config.contracts[0].cases.forEach(c => { c.test.name = "outer & <xml> " + c.test.name; });
  writeFileSync(configPath, JSON.stringify(config));
  assert.equal(caseStatus(run("nested XML-escaped suites", verify, 0), "RateLimited"), "passed");
  writeFileSync(configPath, originalConfig);
  writeFileSync(file, originalTest);

  // Another file's same-named test must not make this case ambiguous.
  writeFileSync(join(work, "tests/other.test.ts"), originalTest);
  run("same names in a different file", verify, 0);
  rmSync(join(work, "tests/other.test.ts"));
  writeFileSync(file, originalTest + '\nit("checkout RateLimited returns 429 without creating an order", () => {});\n');
  assert.equal(caseStatus(run("duplicate identity in same file", verify, 1), "RateLimited"), "ambiguous");
  writeFileSync(file, originalTest);
  writeFileSync(join(work, "tests/setup.test.ts"), 'import { beforeAll, it } from "bun:test"; beforeAll(() => { throw new Error("setup failed"); }); it("unrelated", () => {});');
  const setup = run("unrelated setup failure", verify, 1);
  assert.equal(caseStatus(setup, "RateLimited"), "passed");
  assert(setup.diagnostics.some(d => d.code === "EC006"));

  mkdirSync(join(project, "artifacts"), { recursive: true });
  writeFileSync(join(project, "artifacts/acceptance-bun.json"), JSON.stringify({
    acceptance: "passed", bunVersion: bunVersion.stdout.trim(), scenarios: results,
  }, null, 2).replaceAll(work, "<temporary-project>") + "\n");
  console.log(`All ${results.length} Bun acceptance scenarios passed`);
} finally { rmSync(work, { recursive: true, force: true }); }
