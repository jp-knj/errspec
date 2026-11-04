// Failure paths with real Bun coverage. Mutations stay in a temporary copy.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
const bun = process.argv[3] ? resolve(process.argv[3]) : null;
assert(existsSync(binary), "Run cargo build first");
const base = mkdtempSync(join(tmpdir(), "errspec-failure-paths-"));

try {
  mkdirSync(join(base, "packages"));
  cpSync(join(project, "packages/testkit"), join(base, "packages/testkit"), { recursive: true });
  const work = join(base, "examples/failure-paths");
  cpSync(join(project, "examples/failure-paths"), work, { recursive: true });
  const files = {
    source: join(work, "src/submit.ts"),
    test: join(work, "tests/submit.test.ts"),
    config: join(work, "errspec.json"),
  };
  const original = Object.fromEntries(Object.entries(files).map(([key, path]) => [key, readFileSync(path, "utf8")]));
  const reset = () => Object.entries(files).forEach(([key, path]) => writeFileSync(path, original[key]));
  function run(label, command, expectedExit) {
    const args = command === "verify"
      ? ["verify", "--runner", "bun", "--only-contract-tests", ...(bun ? ["--bun", bun] : [])]
      : ["check"];
    const result = spawnSync(binary, [...args, "--format", "json"], { cwd: work, encoding: "utf8", timeout: 60_000 });
    assert.equal(result.error, undefined, String(result.error));
    assert.equal(result.status, expectedExit, `${label}: ${result.stderr}\n${result.stdout}`);
    console.log(`${label}: exit ${result.status}`);
    return JSON.parse(result.stdout);
  }
  const paths = report => report.contracts[0].paths;
  const codes = report => report.diagnostics.map(d => d.code);

  const listed = run("check lists every return of the entry", "check", 0);
  assert.deepEqual(paths(listed).map(p => p.tags), [["invalid"], ["blocked"], ["blocked"], ["sent"]]);
  assert(paths(listed).every(p => !("reached" in p)));

  const green = run("every path reached by a scenario", "verify", 0);
  assert(paths(green).every(p => p.reached === true), JSON.stringify(paths(green)));

  // Remove the bot scenario: its tag stays registered, but one blocked return is never reached.
  const start = original.test.indexOf('scenarioTest(test, import.meta.url, {\n  contract: "signup", id: "bot"');
  const end = original.test.indexOf("\n});\n", start) + "\n});\n".length;
  assert(start > 0 && end > start);
  writeFileSync(files.test, original.test.slice(0, start) + original.test.slice(end));
  const config = JSON.parse(original.config);
  config.contracts[0].cases = config.contracts[0].cases.filter(c => c.id !== "bot");
  writeFileSync(files.config, JSON.stringify(config));
  const gap = run("an untested return of a registered tag", "verify", 1);
  assert.deepEqual(codes(gap), ["EC019"], JSON.stringify(gap.diagnostics));
  assert.deepEqual(paths(gap).map(p => p.reached), [true, true, false, true]);
  assert.equal(gap.diagnostics[0].file, "src/submit.ts");
  reset();

  // A helper that throws without a catch: the failure escapes outside SubmitResult.
  writeFileSync(files.source, original.source
    .replace('  if (!email.includes("@")) {', '  normalize(email);\n  if (!email.includes("@")) {')
    + '\nfunction normalize(email: string): string {\n  if (email.length > 254) throw new RangeError("address too long");\n  return email.trim();\n}\n');
  const escaped = run("an uncaught throw outside the error type", "check", 1);
  assert.deepEqual(codes(escaped), ["EC018"], JSON.stringify(escaped.diagnostics));
  assert.match(escaped.diagnostics[0].message, /RangeError escape as a rejection via submit -> normalize/);
  reset();

  // The same helper behind a catch is not a definite escape.
  writeFileSync(files.source, original.source
    .replace('  if (!email.includes("@")) {', '  try { normalize(email); } catch { return { status: "invalid" }; }\n  if (!email.includes("@")) {')
    + '\nfunction normalize(email: string): string {\n  if (email.length > 254) throw new RangeError("address too long");\n  return email.trim();\n}\n');
  const caught = run("a caught throw is not an escape", "check", 0);
  assert.equal(paths(caught).length, 5);
  reset();
  console.log("All failure path acceptance checks passed");
} finally {
  rmSync(base, { recursive: true, force: true });
}
