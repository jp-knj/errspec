import {temporaryProject,prepareVitestProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from "node:assert/strict";
import { test } from "node:test";
import {writeFileSync,readFileSync} from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { runMutation } from "../packages/core/index.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = process.env.ERRSPEC_BIN || join(repo, "target/debug/errspec");
function project(
  t,
  source = "export const yes=()=>true; export const no=()=>false;",
) {
  const root = temporaryProject(t,"errspec-workflows-");

  prepareVitestProject(root,repo);
  writeFileSync(join(root, "src/value.ts"), source);
  writeFileSync(
    join(root, "value.test.ts"),
    'import {test,expect} from "vitest";import {yes} from "./src/value";test("value",()=>expect(yes()).toBeDefined());',
  );
  writeFileSync(
    join(root, "vitest.config.mjs"),
    'export default {test:{include:["value.test.ts"]}};',
  );


  return root;
}
function run(root, args = [], expected = 0) {
  return runJsonCommand(binary,["mutate", "--node", process.execPath, "--format", "json", ...args],{ cwd: root, encoding: "utf8", timeout: 60000 },expected);
}
test("explicit comments exclude trials with visible reasons and removing one invalidates cached evidence",t=>{
  const source='// errspec-disable-next-line: intentional weak check\nexport const yes=()=>true;\n/* Stryker disable all: unused API */\nexport const no=()=>false;\n// Stryker restore all\nexport const checked=true;';
  const root=project(t,source);
  writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {yes,checked} from "./src/value";test("value",()=>{expect(yes()).toBeDefined();expect(checked).toBe(true)});');
  const args=['--cache','--reuse-session','--weak-mutation','-j','2','--fail-on-undetected'];
  const cold=run(root,args);assert.equal(cold.summary.ignored,4);assert.equal(cold.summary.killed,1);assert.equal(cold.summary.score,1);
  assert.ok(cold.mutants.filter(m=>m.status==='Ignored').every(m=>m.ignored.startsWith('disabled:')&&m.attempts.length===0));
  const warm=run(root,args);assert.equal(warm.cache.reused,1);assert.equal(warm.cache.executed,0);assert.equal(warm.summary.ignored,4);
  writeFileSync(join(root,'src/value.ts'),source.replace('// errspec-disable-next-line: intentional weak check\n',''));
  const restored=run(root,args,1);assert.equal(restored.cache.reused,0);assert.equal(restored.cache.invalidated,true);assert.equal(restored.summary.ignored,2);assert.equal(restored.summary.survived,1);assert.equal(restored.gate.passed,false);
});
test("arid CLI, configuration and API preserve default trials and explicitly exclude only logging syntax",async t=>{
  const root=project(t,"export function yes(){console.info('telemetry');return true;}");
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {yes} from "./src/value";test("value",()=>expect(yes()).toBe(true));');
  const ordinary=run(root,['--fail-on-undetected'],1);assert.ok(ordinary.summary.survived>0);assert.equal(ordinary.summary.ignored,0);
  writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({arid:true}));
  const excluded=run(root,['--reuse-session','-j','2','--fail-on-undetected']);assert.equal(excluded.summary.ignored,2);assert.equal(excluded.summary.survived,0);assert.equal(excluded.summary.score,1);
  assert.ok(excluded.mutants.filter(m=>m.status==='Ignored').every(m=>m.ignored==='arid: logging-only code'&&m.attempts.length===0));
  const restored=run(root,['--no-arid','--fail-on-undetected'],1);assert.deepEqual(restored.mutants.map(m=>[m.key,m.status]),ordinary.mutants.map(m=>[m.key,m.status]));
  const api=await runMutation({node:process.execPath,arid:false,failOnUndetected:true},{binary,cwd:root});assert.equal(api.exitCode,1);assert.deepEqual(api.report.mutants.map(m=>[m.key,m.status]),restored.mutants.map(m=>[m.key,m.status]));
  const none=await runMutation({node:process.execPath,aridCallees:[],failOnUndetected:true},{binary,cwd:root});assert.equal(none.exitCode,1);assert.equal(none.report.summary.ignored,0);
  for(const pattern of ['', '*', 'console.**', '*.x.*', 'console..log']){
    writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({arid:true,aridCallees:[pattern]}));
    const invalid=run(root,[],2);assert.equal(invalid.testsExecuted,false);assert.equal(invalid.diagnostics[0].code,'EC_MUTATION_CONFIG');
  }
});
test("changing arid patterns invalidates cache and restores unsuppressed candidates",t=>{
  const root=project(t,"export function yes(){console.info('telemetry');return true;}");
  writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {yes} from "./src/value";test("value",()=>expect(yes()).toBe(true));');
  const args=['--cache','--arid','--reuse-session'];
  const cold=run(root,args),warm=run(root,args);assert.equal(cold.summary.ignored,2);assert.ok(warm.cache.reused>0);assert.equal(warm.cache.executed,0);
  const custom=run(root,[...args,'--arid-callee','metrics.*']);assert.equal(custom.cache.reused,0);assert.equal(custom.cache.invalidated,true);assert.equal(custom.summary.ignored,0);assert.ok(custom.summary.survived>0);
});
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
  assert.equal(budget.summary.score, null);
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
test("a detected subset cannot pass full score or undetected gates while candidates remain pending",t=>{
  const root=project(t,"export const yes=(value:number)=>value+1;");
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {yes} from "./src/value";test("boundary",()=>expect(yes(2)).toBe(3));');
  const partial=run(root,["--mutants-per-line","1"]);
  assert.equal(partial.runComplete,true);assert.ok(partial.summary.killed>0);assert.ok(partial.summary.pending>0);
  assert.equal(partial.summary.score,null);
  const score=run(root,["--mutants-per-line","1","--threshold-break","50"],1);
  assert.equal(score.gate.passed,false);assert.equal(score.policy.scoreLevel,null);
  assert.ok(score.policy.qualityFailures.some(reason=>reason.includes("unavailable")));
  const detection=run(root,["--time-budget","0","--fail-on-undetected"],1);
  assert.equal(detection.summary.noCoverage,0);
  assert.ok(detection.summary.pending>0);assert.equal(detection.gate.passed,false);
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
