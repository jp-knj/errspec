import {temporaryProject,prepareVitestProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from "node:assert/strict";
import { test } from "node:test";
import {writeFileSync} from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { attachWeakBinary } from "../packages/mutation-driver/weak-runtime.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = process.env.ERRSPEC_BIN || join(repo, "target/debug/errspec");
const families = [
  "ArithmeticOperator",
  "ArrayDeclaration",
  "ArrowFunction",
  "AssignmentOperator",
  "BlockStatement",
  "BooleanLiteral",
  "CallExpression",
  "ConditionalExpression",
  "FnValue",
  "LogicalOperator",
  "MethodExpression",
  "ObjectLiteral",
  "OptionalChaining",
  "Regex",
  "StringLiteral",
  "UnaryOperator",
  "UpdateOperator",
];

test("weak probes evaluate operands and object coercions once and retain exceptional alternatives", () => {
  const reached = [],
    infected = [];
  const runtime = attachWeakBinary(
    (key) => {
      reached.push(key);
      return false;
    },
    (key) => infected.push(key),
    null,
  );
  assert.equal(runtime.binary(">=", [["greater", ">"]], 19, 18), true);
  assert.deepEqual(infected, []);
  assert.equal(runtime.binary(">=", [["greater", ">"]], 18, 18), true);
  assert.deepEqual(infected, ["greater"]);
  let coercions = 0;
  const object = {
    valueOf() {
      coercions++;
      return 19;
    },
  };
  assert.equal(runtime.binary(">=", [["object", ">"]], object, 18), true);
  assert.equal(coercions, 1);
  assert.ok(infected.includes("object"));
  assert.equal(runtime.binary("*", [["division", "/"]], 1n, 0n), 0n);
  assert.ok(infected.includes("division"));
  const active = attachWeakBinary(
    (key) => key === "greater",
    () => assert.fail("Active run must not collect weak evidence"),
    "greater",
  );
  assert.equal(active.binary(">=", [["greater", ">"]], 18, 18), false);
  assert.deepEqual(reached, ["greater", "greater", "object", "division"]);
});

function project(t, tests) {
  const root = temporaryProject(t,"errspec-weak-");

  prepareVitestProject(root,repo);
  writeFileSync(
    join(root, "src/value.ts"),
    "export function age(x:number){return x>=18;}",
  );
  writeFileSync(
    join(root, "value.test.ts"),
    `import {test,expect} from 'vitest';import {age} from './src/value';${tests}`,
  );
  writeFileSync(
    join(root, "vitest.config.mjs"),
    "export default {test:{include:['value.test.ts']}};",
  );


  return root;
}
function run(root, weak) {
  const args = [
    "mutate",
    ...(process.env.ERRSPEC_REUSE_SESSION?["--reuse-session"]:[]),
    "--node",
    process.execPath,
    "--format",
    "json",
    weak ? "--weak-mutation" : "--no-weak-mutation",
    ...families.flatMap((name) => ["--exclude-mutator", name]),
  ];
  return runJsonCommand(binary,args,{
    cwd: root,
    encoding: "utf8",
    timeout: 60000,
  },0);
}
function greater(report) {
  return report.mutants.find(
    (m) => m.replacement.includes(" > ") || m.replacement === "x>18",
  );
}

test("weak mutation selects only tests that observe a value difference", (t) => {
  const root = project(
    t,
    "test('over',()=>expect(age(19)).toBe(true));test('edge',()=>expect(age(18)).toBe(true));",
  );
  const report = run(root, true),
    mutant = greater(report);
  assert.equal(mutant.status, "Killed");
  assert.equal(mutant.coveredBy.length, 2);
  assert.equal(mutant.infectedBy.length, 1);
  assert.ok(mutant.infectedBy[0].includes("edge"));
  assert.equal(mutant.attempts[0].killedBy.length, 1);
});

test("not infected is explicit and disabling weak mutation executes the same candidate", (t) => {
  const root = project(t, "test('over',()=>expect(age(19)).toBe(true));");
  const weak = greater(run(root, true));
  assert.equal(weak.status, "Survived");
  assert.match(weak.reason, /not infected/);
  assert.deepEqual(weak.infectedBy, []);
  assert.deepEqual(weak.attempts, []);
  const strong = greater(run(root, false));
  assert.equal(strong.status, "Survived");
  assert.equal(strong.attempts.length, 1);
  assert.equal(strong.infectedBy, undefined);
});
