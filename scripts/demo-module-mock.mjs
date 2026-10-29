// Faults reached through module mocks with real Bun/Vitest. Mutations stay in a temporary copy.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
const bun = process.argv[3] ? resolve(process.argv[3]) : null;
assert(existsSync(binary), "Run cargo build first");
const base = mkdtempSync(join(tmpdir(), "errspec-module-mock-"));

try {
  mkdirSync(join(base, "packages"));
  cpSync(join(project, "packages/testkit"), join(base, "packages/testkit"), { recursive: true });
  const work = join(base, "examples/module-mock");
  cpSync(join(project, "examples/module-mock"), work, { recursive: true });
  symlinkSync(join(project, "examples/checkout/node_modules"), join(work, "node_modules"), "dir");

  for (const runner of ["bun", "vitest"]) {
    const verify = ["verify", "--runner", runner, "--config", `errspec.${runner}.json`, "--only-contract-tests",
      ...(runner === "bun" && bun ? ["--bun", bun] : [])];
    const file = join(work, `tests/notify.${runner}.test.ts`);
    const original = readFileSync(file, "utf8");
    function run(label, expectedExit) {
      const result = spawnSync(binary, [...verify, "--format", "json"], { cwd: work, encoding: "utf8", timeout: 60_000 });
      assert.equal(result.error, undefined, String(result.error));
      assert.equal(result.status, expectedExit, `${runner}/${label}: ${result.stderr}\n${result.stdout}`);
      console.log(`${runner}/${label}: exit ${result.status}`);
      return JSON.parse(result.stdout);
    }
    function unobserved(report) {
      const faults = report.diagnostics.filter(d => d.code === "EC009");
      assert.equal(faults.length, 3, JSON.stringify(report.diagnostics));
    }

    const green = run("faults reached through the module mock", 0);
    assert(green.contracts[0].cases.every(c => c.observationStatus === "passed"));

    const mockCall = runner === "bun" ? /^mock\.module\(.*\n/m : /^vi\.mock\(.*\n/m;
    assert.match(original, mockCall);
    writeFileSync(file, original.replace(mockCall, ""));
    unobserved(run("without the module mock the real dependency runs", 1));

    writeFileSync(file, original);
  }
} finally {
  rmSync(base, { recursive: true, force: true });
}
