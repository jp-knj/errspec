import assert from "node:assert/strict";
import { test } from "node:test";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  rmSync,
  symlinkSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = process.env.ERRSPEC_BIN || join(repo, "target/debug/errspec");
function project(
  t,
  source = "export const yes=()=>true; export const no=()=>false;",
) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "errspec-workflows-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "src"));
  writeFileSync(join(root, "src/value.ts"), source);
  writeFileSync(
    join(root, "value.test.ts"),
    'import {test,expect} from "vitest";import {yes} from "./src/value";test("value",()=>expect(yes()).toBeDefined());',
  );
  writeFileSync(
    join(root, "vitest.config.mjs"),
    'export default {test:{include:["value.test.ts"]}};',
  );
  mkdirSync(join(root, "node_modules"));
  symlinkSync(
    join(repo, "examples/checkout/node_modules/vitest"),
    join(root, "node_modules/vitest"),
    "dir",
  );
  return root;
}
function run(root, args = [], expected = 0) {
  const output = spawnSync(
    binary,
    ["mutate", "--node", process.execPath, "--format", "json", ...args],
    { cwd: root, encoding: "utf8", timeout: 60000 },
  );
  assert.ifError(output.error);
  assert.equal(output.status, expected, output.stderr + output.stdout);
  return JSON.parse(output.stdout);
}
test("only IDs, deterministic sampling, and time budgets retain explicit pending states", (t) => {
  const root = project(t),
    original = readFileSync(join(root, "src/value.ts"), "utf8"),
    base = run(root);
  assert.equal(base.summary.survived, 1);
  assert.equal(base.summary.noCoverage, 2);
  const key = base.mutants.find((m) => m.status === "Survived").key;
  const selected = run(root, ["--only", key]);
  assert.deepEqual(
    selected.mutants.map((m) => m.key),
    [key],
  );
  const sampled = run(root, ["--mutants-per-line", "1"]);
  assert.equal(sampled.policy.sampled, 1);
  assert.equal(sampled.summary.pending, 1);
  const again = run(root, ["--mutants-per-line", "1"]);
  assert.deepEqual(
    sampled.mutants.map((m) => [m.key, m.status]),
    again.mutants.map((m) => [m.key, m.status]),
  );
  const budget = run(root, ["--time-budget", "0"]);
  assert.equal(budget.summary.pending, 2);
  assert.equal(budget.summary.noCoverage, 2);
  assert.ok(
    budget.mutants
      .filter((m) => m.status === "Pending")
      .every(
        (m) =>
          m.reason.includes("budget") &&
          m.attempts.length === 0 &&
          m.coveredBy.length > 0,
      ),
  );
  assert.equal(readFileSync(join(root, "src/value.ts"), "utf8"), original);
  assert.equal(run(root, ["--only", "a".repeat(64)], 2).runComplete, false);
});
test("score thresholds, survival gates and critical globs fail independently of the baseline", (t) => {
  const root = project(t);
  const score = run(root, ["--threshold-break", "90"], 1);
  assert.equal(score.runComplete, true);
  assert.equal(score.gate.passed, false);
  assert.equal(score.policy.scoreLevel, "danger");
  assert.ok(score.diagnostics.some((d) => d.code === "EC_MUTATION_QUALITY"));
  const survived = run(root, ["--fail-on-survived"], 1);
  assert.ok(survived.policy.qualityFailures[0].includes("survived"));
  const critical = run(root, ["--critical", "src/**", "--time-budget", "0"], 1);
  assert.ok(
    critical.policy.qualityFailures.some((message) =>
      message.includes("critical"),
    ),
  );
  assert.ok(critical.mutants.every((m) => m.critical));
});
test("critical comments apply to their declaration and do not leak into another function", (t) => {
  const root = project(
    t,
    "// errspec-critical\nexport const yes=()=>true;\nexport const no=()=>false;\n",
  );
  const result = run(root, [], 1);
  assert.equal(result.mutants.filter((m) => m.critical).length, 2);
  assert.equal(result.mutants.filter((m) => !m.critical).length, 2);
  assert.ok(
    result.policy.qualityFailures.some((message) =>
      message.includes("critical"),
    ),
  );
});
test("reasoned approvals are automatic, stale IDs are reported, and empty reasons fail", (t) => {
  const root = project(t),
    base = run(root);
  const accepted = Object.fromEntries(
    base.mutants
      .filter((m) => ["Survived", "NoCoverage"].includes(m.status))
      .map((m) => [m.key, { reason: "Equivalent for the documented domain" }]),
  );
  accepted["f".repeat(64)] = { reason: "Old candidate" };
  writeFileSync(join(root, "errspec.accepted.json"), JSON.stringify(accepted));
  const result = run(root, ["--fail-on-undetected"]);
  assert.equal(result.summary.survived, 0);
  assert.equal(result.summary.noCoverage, 0);
  assert.equal(result.policy.acceptance.applied.length, 3);
  assert.deepEqual(result.policy.acceptance.stale, ["f".repeat(64)]);
  assert.equal(result.summary.score, 1);
  writeFileSync(
    join(root, "errspec.accepted.json"),
    JSON.stringify({ [base.mutants[0].key]: { reason: " " } }),
  );
  const invalid = run(root, [], 2);
  assert.equal(invalid.testsExecuted, false);
  assert.equal(invalid.runComplete, false);
});

test("accept command preserves earlier approvals and refuses corrupt or reasonless updates", (t) => {
  const root = project(t),
    base = run(root),
    keys = base.mutants.slice(0, 2).map((m) => m.key);
  const accept = (key, reason, expected) => {
    const result = spawnSync(
      binary,
      ["accept", key, "--reason", reason, "--format", "json"],
      { cwd: root, encoding: "utf8" },
    );
    assert.equal(result.status, expected, result.stdout + result.stderr);
    return JSON.parse(result.stdout);
  };
  assert.equal(accept(keys[0], "Reason one", 0).ok, true);
  assert.equal(accept(keys[1], "Reason two", 0).ok, true);
  const file = join(root, "errspec.accepted.json"),
    approved = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(approved[keys[0]].reason, "Reason one");
  assert.equal(approved[keys[1]].reason, "Reason two");
  const before = readFileSync(file, "utf8");
  assert.equal(accept(keys[0], " ", 2).ok, false);
  assert.equal(readFileSync(file, "utf8"), before);
  writeFileSync(file, "{broken");
  assert.equal(accept(keys[0], "Reason", 2).ok, false);
  assert.equal(readFileSync(file, "utf8"), "{broken");
});
