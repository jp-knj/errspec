// Exercise untouched init output with each real runner, standalone and through verify.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
const bun = process.argv[3] ? resolve(process.argv[3]) : null;
const dependencies = join(project, "examples/checkout/node_modules");
const vitest = join(dependencies, ".bin/vitest");
const base = mkdtempSync(join(tmpdir(), "errspec-init-"));
const checks = [];
const contract = 'checkout "日本語" & <xml>\n';
const tags = ["Denied", 'Off"line\\\n日本語'];
const testFile = "tests/generated 日本語.test.ts";
const environment = { ...process.env };
delete environment.ERRSPEC_OBSERVATIONS;
delete environment.ERRSPEC_RUN_ID;

function execute(work, command, args, expected) {
  const result = spawnSync(command, args, { cwd: work, env: environment, encoding: "utf8", timeout: 60_000 });
  assert.equal(result.error, undefined, String(result.error));
  assert.equal(result.status, expected, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result;
}

try {
  // Resolve Vitest from the pinned installation, without modifying it.
  symlinkSync(dependencies, join(base, "node_modules"), "dir");
  for (const runner of ["bun", "vitest"]) {
    const work = join(base, runner);
    mkdirSync(join(work, "tests"), { recursive: true });
    mkdirSync(join(work, "node_modules/@errspec"), { recursive: true });
    symlinkSync(join(project, "packages/testkit"), join(work, "node_modules/@errspec/testkit"), "dir");
    writeFileSync(join(work, "package.json"), JSON.stringify({ type: "module" }));
    writeFileSync(join(work, "errors.ts"), "export type Failure = " + tags.map(tag => `{ _tag: ${JSON.stringify(tag)} }`).join(" | ") + ";\n");
    writeFileSync(join(work, "tsconfig.json"), JSON.stringify({ compilerOptions: { strict: true, skipLibCheck: true }, files: ["errors.ts"] }));
    function cli(label, args, exit) {
      const result = execute(work, binary, [...args, "--format", "json"], exit);
      const report = JSON.parse(result.stdout);
      checks.push({ runner, label, exit, report });
      console.log(`${runner}/${label}: exit ${exit}`);
      return report;
    }
    const generated = cli("generate escaped identities", ["init", "--source", "errors.ts", "--export", "Failure", "--runner", runner, "--contract", contract, "--test", testFile], 0);
    assert.equal(generated.ready, false);
    assert.equal(generated.testsExecuted, false);
    assert.deepEqual(generated.tags, tags);
    const config = JSON.parse(readFileSync(join(work, "errspec.json"), "utf8"));
    const cases = config.contracts[0].cases;
    assert.equal(config.contracts[0].id, contract);
    assert.deepEqual(cases.map(c => [c.id, c.tag, c.test.file]), [["case-1", tags[0], testFile], ["case-2", tags[1], testFile]]);
    const checked = cli("generated registrations match", ["check"], 0);
    assert(checked.contracts[0].cases.every(c => c.testStatus === "not_run" && c.observationStatus === "not_run"));

    const reportFile = join(work, runner === "bun" ? "standalone.xml" : "standalone.json");
    const standalone = execute(work, runner === "bun" ? bun ?? "bun" : vitest,
      runner === "bun" ? ["test", "--reporter=junit", "--reporter-outfile", reportFile, testFile]
        : ["run", "--reporter=json", "--outputFile", reportFile, testFile], 1);
    if (runner === "vitest") {
      const report = JSON.parse(readFileSync(reportFile, "utf8"));
      const tests = report.testResults.flatMap(file => file.assertionResults);
      assert.equal(report.success, false);
      assert.deepEqual(tests.map(t => [t.fullName, t.status]), cases.map(c => [`errspec ${JSON.stringify([contract, c.id])}`, "failed"]));
      assert(tests.some(t => t.failureMessages.some(message => message.includes("TODO: implement the Denied failure scenario"))));
    } else {
      assert((standalone.stdout + standalone.stderr).includes("TODO: implement the Denied failure scenario"));
      const xml = readFileSync(reportFile, "utf8");
      assert.equal((xml.match(/<testcase\b/g) ?? []).length, 2);
      assert.equal((xml.match(/<failure\b/g) ?? []).length, 2);
    }
    checks.push({ runner, label: "standalone TODOs fail", exit: 1, failedTests: 2 });
    console.log(`${runner}/standalone TODOs fail: exit 1`);

    const executable = runner === "vitest" ? ["--vitest", vitest] : bun ? ["--bun", bun] : [];
    const verified = cli("generated TODOs cannot verify", ["verify", "--runner", runner, ...executable, "--only-contract-tests"], 1);
    assert.equal(verified.analysisComplete, true);
    assert.equal(verified.testsExecuted, true);
    assert.equal(verified.testRunner, runner);
    assert.deepEqual(verified.contracts[0].cases.map(c => [c.id, c.tag, c.testStatus, c.observationStatus]), cases.map(c => [c.id, c.tag, "failed", "failed"]));
  }
  mkdirSync(join(project, "artifacts"), { recursive: true });
  writeFileSync(join(project, "artifacts/acceptance-init.json"), JSON.stringify({ acceptance: "passed", checks }, null, 2).replaceAll(base, "<temporary-project>") + "\n");
  console.log(`All ${checks.length} init acceptance checks passed`);
} finally {
  rmSync(base, { recursive: true, force: true });
}
