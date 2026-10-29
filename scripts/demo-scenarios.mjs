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
const base = mkdtempSync(join(tmpdir(), "errspec-scenarios-"));
const all = [];

try {
  mkdirSync(join(base, "packages"));
  cpSync(join(project, "packages/testkit"), join(base, "packages/testkit"), { recursive: true });
  for (const runner of ["bun", "vitest"]) {
    const work = join(base, "examples", `scenarios-${runner}`);
    cpSync(join(project, "examples", `scenarios-${runner}`), work, { recursive: true, filter: p => !p.split("/").includes("node_modules") });
    if (runner === "vitest") symlinkSync(join(project, "examples/checkout/node_modules"), join(work, "node_modules"), "dir");
    const verify = ["verify", "--runner", runner, ...(runner === "bun" && bun ? ["--bun", bun] : [])];
    const file = join(work, "tests/checkout.test.ts");
    const source = join(work, "src/checkout.ts");
    const configPath = join(work, "errspec.json");
    const original = readFileSync(file, "utf8");
    const originalSource = readFileSync(source, "utf8");
    const originalConfig = readFileSync(configPath, "utf8");
    function reset() {
      writeFileSync(file, original); writeFileSync(source, originalSource); writeFileSync(configPath, originalConfig);
    }
    function run(label, expectedExit = 1, args = verify) {
      const result = spawnSync(binary, [...args, "--format", "json"], { cwd: work, encoding: "utf8", timeout: 60_000 });
      assert.equal(result.error, undefined, String(result.error));
      assert.equal(result.status, expectedExit, `${runner}/${label}: ${result.stderr}\n${result.stdout}`);
      const report = JSON.parse(result.stdout);
      all.push({ runner, label, exitCode: result.status, report });
      console.log(`${runner}/${label}: exit ${result.status}`);
      return report;
    }
    function has(report, code) { assert(report.diagnostics.some(d => d.code === code), `${code}: ${JSON.stringify(report)}`); }
    const cases = report => report.contracts[0].cases;
    const green = run("three scenarios for two tags", 0);
    assert.equal(cases(green).length, 3);
    assert(cases(green).every(c => c.observationStatus === "passed" && c.testStatus === "passed"));
    assert.equal(cases(green).find(c => c.id === "rate-limit-retries").observedFaults[0].calls, 3);
    assert(cases(run("check does not imply observed", 0, ["check"])).every(c => c.observationStatus === "not_run"));
    run("snapshot includes scenario inventory", 0, ["snapshot", "--out", "baseline.json"]);
    const fewer = JSON.parse(originalConfig);
    fewer.contracts[0].cases.pop();
    writeFileSync(configPath, JSON.stringify(fewer));
    const removed = run("removing a scenario while keeping its tag", 1, ["check", "--baseline", "baseline.json"]);
    has(removed, "EC013");
    assert.deepEqual(removed.delta.removedScenarios["checkout.payment"], ["rate-limit-retries"]);
    reset();

    writeFileSync(file, original.replaceAll('s.check("orders", orders, 0);', '/* assertion accidentally removed */'));
    const missingCheck = run("passing tests with missing required assertion");
    assert(cases(missingCheck).every(c => c.testStatus === "passed"));
    has(missingCheck, "EC010"); reset();

    writeFileSync(file, original.replaceAll("const response = await checkout({ charge, createOrder: async () => { orders++; } }, maxAttempts);", "const response = { status: 429 }; attempts = maxAttempts;"));
    has(run("configured fault never invoked"), "EC009"); reset();

    writeFileSync(source, originalSource.replace("status: 429", "status: 200"));
    has(run("incorrect error response"), "EC010"); reset();
    writeFileSync(source, originalSource.replace('if (attempt >= maxAttempts) return { status: 429 };', 'if (attempt >= maxAttempts) { await deps.createOrder(); return { status: 429 }; }'));
    has(run("unexpected order side effect"), "EC010"); reset();
    writeFileSync(source, originalSource.replace("attempt >= maxAttempts", "attempt >= 1"));
    has(run("retry stops too early"), "EC010"); reset();

    writeFileSync(file, original.replaceAll('s.check("orders", orders, 0);', 'try { s.check("orders", orders, 1); } catch {}'));
    has(run("caught failed assertion remains failed"), "EC010"); reset();
    writeFileSync(file, original.replaceAll('id, tag: "RateLimited"', 'id, tag: "Declined"'));
    const mismatch = run("wrong error tag in SDK record");
    assert(cases(mismatch).every(c => c.testStatus === "passed")); has(mismatch, "EC011"); reset();

    writeFileSync(file, original.replaceAll("scenarioTest(test,", "scenarioTest(test.concurrent,"));
    run("concurrent scenario isolation", 0); reset();
    const escapedId = 'declined "日本語" & <xml>';
    const escapedConfig = JSON.parse(originalConfig);
    escapedConfig.contracts[0].cases[0].id = escapedId;
    writeFileSync(configPath, JSON.stringify(escapedConfig));
    writeFileSync(file, original.replace('id: "declined"', `id: ${JSON.stringify(escapedId)}`));
    run("unicode and escaped stable identities", 0); reset();
    writeFileSync(file, original.replaceAll("scenarioTest(test,", "scenarioTest(test.skip,"));
    has(run("skipped scenarios have no evidence"), "EC008"); reset();

    const runnerImport = runner === "bun" ? "bun:test" : "vitest";
    const config = JSON.parse(originalConfig);
    writeFileSync(file, `import { test } from ${JSON.stringify(runnerImport)};\n` + config.contracts[0].cases.map(c =>
      `test(${JSON.stringify("errspec " + JSON.stringify(["checkout.payment", c.id]))}, () => {});`
    ).join("\n"));
    const empty = run("matching passing tests without SDK");
    assert(cases(empty).every(c => c.testStatus === "passed")); has(empty, "EC008"); reset();

    // The body runs twice under the same stable id: a retry/repeat is not silently accepted.
    writeFileSync(file, original + original.slice(original.indexOf("scenarioTest(test,")));
    has(run("duplicate scenario execution"), "EC011"); reset();

    writeFileSync(file, original + '\nimport { writeFileSync } from "node:fs";\nimport { join } from "node:path";\nwriteFileSync(join(process.env.ERRSPEC_OBSERVATIONS!, "corrupt.json"), "{");\n');
    has(run("corrupt observation cannot pass", 2), "EC_EVIDENCE"); reset();
  }
  mkdirSync(join(project, "artifacts"), { recursive: true });
  writeFileSync(join(project, "artifacts/acceptance-scenarios.json"), JSON.stringify({ acceptance: "passed", scenarios: all }, null, 2).replaceAll(base, "<temporary-project>") + "\n");
  console.log(`All ${all.length} scenario acceptance checks passed`);
} finally { rmSync(base, { recursive: true, force: true }); }
