// Mutation acceptance checks use real Vitest workers and temporary projects.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, symlinkSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

assert(Number(process.versions.node.split(".")[0])>=24,"Run this script with Node 24+");
const project=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const binary=resolve(process.argv[2]||join(project,"target/debug/errspec"));
const base=mkdtempSync(join(tmpdir(),"errspec-mutation-acceptance-"));
const families=["ArithmeticOperator","ArrayDeclaration","ArrowFunction","AssignmentOperator","BlockStatement","BooleanLiteral","CallExpression","ConditionalExpression","EqualityOperator","FnValue","LogicalOperator","MethodExpression","ObjectLiteral","OptionalChaining","Regex","StringLiteral","UnaryOperator","UpdateOperator"];
let sequence=0;
function fixture(files) {
  const work=join(base,String(++sequence));mkdirSync(work);
  const all={"package.json":'{"type":"module"}',...files};
  for(const [file,source] of Object.entries(all)) {mkdirSync(dirname(join(work,file)),{recursive:true});writeFileSync(join(work,file),source);}
  symlinkSync(join(project,"examples/checkout/node_modules"),join(work,"node_modules"),"dir");
  return work;
}
function run(work,only,args=[],expected=0) {
  const exclusions=families.filter(f=>!only.includes(f)).flatMap(f=>["--exclude-mutator",f]);
  const result=spawnSync(binary,["mutate","--root",work,"--node",process.execPath,"--format","json",...(process.env.ERRSPEC_REUSE_SESSION?["--reuse-session"]:[]),...exclusions,...args],{encoding:"utf8",timeout:120000});
  assert.equal(result.error,undefined,String(result.error));
  assert.equal(result.status,expected,`${result.stderr}\n${result.stdout}`);
  const report=JSON.parse(result.stdout);
  assert.equal(report.kind,"errspec/mutation");assert.equal(report.schemaVersion,1);
  if(expected<2) {assert.equal(report.runComplete,true);assert.equal(report.testsExecuted,true);}
  else {assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);assert.equal(report.gate.passed,null);}
  return report;
}
const imports='import {test,expect} from "vitest";\n';
try {
  const work=fixture({"src/age.ts":"export const adult=(age:number)=>age>=18;\n","src/unimported.ts":"export const old=(age:number)=>age>=65;\n",
    "age.test.ts":imports+'import {adult} from "./src/age";test("adult",()=>expect(adult(19)).toBe(true));'});
  const before=readFileSync(join(work,"src/age.ts"),"utf8");
  const informational=run(work,["EqualityOperator"]);
  assert.equal(informational.summary.survived,1);assert.equal(informational.summary.killed,1);assert.equal(informational.summary.noCoverage,2);
  assert.equal(informational.gate.passed,null);assert.equal(informational.ok,true);
  assert(informational.mutants.filter(m=>m.status==="NoCoverage").every(m=>m.attempts.length===0));
  const strict=run(work,["EqualityOperator"],["--fail-on-undetected"],1);
  assert.equal(strict.gate.passed,false);assert(strict.diagnostics.some(d=>d.code==="EC_MUTATION_UNDETECTED"));
  assert.deepEqual(strict.mutants.map(m=>m.key),informational.mutants.map(m=>m.key));
  assert.equal(readFileSync(join(work,"src/age.ts"),"utf8"),before);
  console.log("mutation: killed, survived, uncovered, stable keys, and optional gate");

  const duplicate=fixture({"src/age.ts":"export const adult=(age:number)=>age>=18;",
    "age.test.ts":imports+'import {adult} from "./src/age";test("same",()=>expect(adult(19)).toBe(true));test("same",()=>expect(adult(18)).toBe(true));test("unrelated",()=>expect(1).toBe(1));'});
  const duplicates=run(duplicate,["EqualityOperator"]);
  const boundary=duplicates.mutants.find(m=>m.replacement==="age>18");
  assert(boundary);assert.equal(boundary.coveredBy.length,2);assert.equal(boundary.killedBy.length,1);
  assert.equal(JSON.parse(boundary.killedBy[0])[3],1);
  assert.equal(duplicates.baseline.length,3);
  console.log("mutation: exact duplicate-name registration identities and per-test selection");

  const staticWork=fixture({"src/value.ts":"export const value=1+2;",
    "value.test.ts":imports+'import {value} from "./src/value";test("static",()=>expect(value).toBe(3));test("also",()=>expect(value).toBeGreaterThan(0));'});
  const staticReport=run(staticWork,["ArithmeticOperator"]);
  assert.equal(staticReport.mutants[0].coveredBy.length,2);assert.equal(staticReport.mutants[0].status,"Killed");
  console.log("mutation: static initialization conservatively selects the entire test file");

  const runtime=fixture({"src/cases.ts":"export const cases=[1,2];",
    "cases.test.ts":imports+'import {cases} from "./src/cases";test.each(cases)("case %s",n=>expect(n).toBeGreaterThan(0));'});
  const runtimeReport=run(runtime,["ArrayDeclaration"]);
  assert.equal(runtimeReport.mutants[0].status,"RuntimeError");assert.equal(runtimeReport.mutants[0].attempts.length,2);
  assert(runtimeReport.mutants[0].failedModules.includes("cases.test.ts"));
  console.log("mutation: disappeared registrations are confirmed runtime errors");

  const looping=fixture({"src/loop.ts":"export function count(){let n=0;while(n<1){n++;}return n;}",
    "loop.test.ts":imports+'import {count} from "./src/loop";test("count",()=>expect(count()).toBe(1));'});
  const loopReport=run(looping,["UpdateOperator"]);
  assert.equal(loopReport.mutants[0].status,"Timeout");assert.equal(loopReport.mutants[0].attempts.length,1);
  console.log("mutation: hit-limit detection terminates synchronous loops");

  const failed=fixture({"src/x.ts":"export const value=true;","x.test.ts":imports+'test("fails",()=>expect(1).toBe(2));'});
  const failedReport=run(failed,["BooleanLiteral"],[],2);
  assert.equal(failedReport.testsExecuted,true);assert.equal(failedReport.mutants.length,0);
  assert(failedReport.diagnostics.some(d=>d.code==="EC_MUTATION_BASELINE"));
  const skipped=fixture({"src/x.ts":"export const value=true;","x.test.ts":imports+'test.skip("skip",()=>{});'});
  assert.equal(run(skipped,["BooleanLiteral"],[],2).testsExecuted,false);
  const snapshot=fixture({"src/x.ts":"export const value=true;","x.test.ts":imports+'test("snapshot",()=>expect(1).toMatchSnapshot());'});
  run(snapshot,["BooleanLiteral"],[],2);
  assert.equal(existsSync(join(snapshot,"__snapshots__")),false);
  console.log("mutation: failing and empty baselines stop; snapshot files are not created");

  const concurrent=fixture({"src/x.ts":"export const value=(n:number)=>n+1;",
    "x.test.ts":imports+'import {value} from "./src/x";test.concurrent("a",async()=>expect(value(1)).toBe(2));test.concurrent("b",async()=>expect(1).toBe(1));'});
  const concurrentReport=run(concurrent,["ArithmeticOperator"]);
  assert.equal(concurrentReport.mutants[0].coveredBy.length,2);
  console.log("mutation: concurrent tests use conservative file coverage");

  const logged=fixture({"src/x.ts":"export const value=(n:number)=>n+1;",
    "vitest.config.mjs":'console.log("config log");export default {test:{include:["x.test.ts"]}};',
    "x.test.ts":imports+'import {value} from "./src/x";test("log",()=>{console.log("test log");expect(value(1)).toBe(2)});'});
  assert.equal(run(logged,["ArithmeticOperator"]).mutants[0].status,"Killed");
  console.log("mutation: config and test logs leave JSON stdout intact");

  const workspace=fixture({"src/x.ts":"export const value=(n:number)=>n+1;",
    "vitest.config.mjs":'export default {test:{projects:[{test:{name:"api",include:["api.test.ts"]}},{test:{name:"web",include:["web.test.ts"]}}]}};',
    "api.test.ts":imports+'import {value} from "./src/x";test("api",()=>expect(value(1)).toBe(2));',
    "web.test.ts":imports+'test("web",()=>expect(1).toBe(1));'});
  assert(run(workspace,["ArithmeticOperator"],[],2).diagnostics.some(d=>d.code==="EC_MUTATION_CONFIG"));
  const selectedProject=run(workspace,["ArithmeticOperator"],["--vitest-project","api"]);
  assert.equal(selectedProject.baseline.length,1);assert.equal(selectedProject.mutants[0].status,"Killed");
  assert.equal(JSON.parse(selectedProject.baseline[0].id)[0],"api");
  console.log("mutation: ambiguous workspaces require one selected project");

  const setupProject=fixture({"src/x.ts":"export const value=(n:number)=>n+1;",
    "global.mjs":'import {value} from "./src/x";export default ()=>{if(value(1)!==2)throw new Error("global setup changed");};',
    "vitest.config.mjs":'export default {test:{globalSetup:["./global.mjs"],setupFiles:["./user-setup.ts"]}};',
    "user-setup.ts":'import {value} from "./src/x";if(value(1)!==2)throw new Error("mutated worker setup");',
    "x.test.ts":imports+'import {value} from "./src/x";test("setup",()=>expect(value(1)).toBe(2));'});
  const setupReport=run(setupProject,["ArithmeticOperator"]);
  assert.equal(setupReport.mutants[0].status,"RuntimeError");
  assert.equal(setupReport.mutants[0].attempts.length,2);
  console.log("mutation: worker probes precede user setup; global setup retains unmodified behavior");

  const timeout=fixture({"src/x.ts":"export const delay=false;",
    "vitest.config.mjs":'export default {test:{testTimeout:30}};',
    "x.test.ts":imports+'import {delay} from "./src/x";test("delay",async()=>{await new Promise(resolve=>setTimeout(resolve,delay?60:1));expect(1).toBe(1)});'});
  const timeoutReport=run(timeout,["BooleanLiteral"]);
  assert.equal(timeoutReport.mutants[0].attempts.length,2);assert.equal(timeoutReport.mutants[0].attempts[0].status,"Timeout");
  assert.equal(timeoutReport.mutants[0].status,"Survived");
  console.log("mutation: timeout confirmation uses three times the runner budget");

  const excluded=run(work,["EqualityOperator"],["--exclude","src/age.ts","--include","src/**/*","--include","src/age.ts"]);
  assert(excluded.mutants.every(m=>m.file==="src/unimported.ts"));
  assert.equal(excluded.summary.noCoverage,2);
  assert(run(work,["EqualityOperator"],["--config","errspec.json"],2).diagnostics.some(d=>d.code==="EC_MUTATION_CONFIG"));
  console.log("mutation: include and exclude are rooted at the target project; contract config is rejected");

  const changed=fixture({"src/x.ts":"export const value=true;","x.test.ts":imports+
    'import {writeFileSync} from "node:fs";test("changes input",()=>{writeFileSync(new URL("./src/x.ts",import.meta.url),"export const value=false;")});'});
  assert(run(changed,["BooleanLiteral"],[],2).diagnostics.some(d=>d.code==="EC_MUTATION_INPUT"));
  console.log("mutation: changing an input invalidates the run");

  const probeChanged=fixture({"src/x.ts":"export function value(n:number){return n+1;}",
    "x.test.ts":imports+'import {value} from "./src/x";test("source text",()=>{expect(String(value)).not.toContain("errspec.mutation");expect(value(1)).toBe(2)});'});
  const probeChangedReport=run(probeChanged,["ArithmeticOperator"],[],2);
  assert(probeChangedReport.diagnostics.some(d=>d.code==="EC_MUTATION_INSTRUMENTATION"));
  assert(probeChangedReport.mutants.every(m=>m.attempts.length===0));
  console.log("mutation: probe baseline differences invalidate instrumentation");

  const earlierPlugin=fixture({"src/x.ts":"export const value=(n:number)=>n+1;",
    "vitest.config.mjs":'export default {plugins:[{name:"earlier",enforce:"pre",transform(code,id){if(id.endsWith("/src/x.ts"))return code.replace("n+1","n+2");}}]};',
    "x.test.ts":imports+'import {value} from "./src/x";test("plugin",()=>expect(value(1)).toBe(3));'});
  assert(run(earlierPlugin,["ArithmeticOperator"],[],2).diagnostics.some(d=>d.code==="EC_MUTATION_INSTRUMENTATION"));
  console.log("mutation: earlier plugin transforms cannot invalidate source ranges silently");

  const brokenMetadata=fixture({"src/x.ts":"export const value=true;",
    "x.test.ts":imports+'import {TestRunner} from "vitest";test("bad metadata",()=>{TestRunner.getCurrentTest().meta.__errspecMutation={runId:"other",requestId:0,hits:{}};expect(1).toBe(1)});'});
  assert(run(brokenMetadata,["BooleanLiteral"],[],2).diagnostics.some(d=>d.code==="EC_MUTATION_PROTOCOL"));
  console.log("mutation: wrong run metadata is a protocol error");

  const invalid=run(work,["UnknownMutator"],["--exclude-mutator","UnknownMutator"],2);
  assert(invalid.diagnostics.some(d=>d.code==="EC_MUTATION_CONFIG"));
  const wrongVersion=fixture({"vendor/vitest/package.json":'{"name":"vitest","version":"4.0.0"}'});
  const versionReport=run(work,["EqualityOperator"],["--vitest-package",join(wrongVersion,"vendor/vitest")],2);
  assert(versionReport.diagnostics.some(d=>d.code==="EC_MUTATION_CONFIG"));assert.equal(versionReport.testsExecuted,false);
  console.log("mutation: invalid configuration is incomplete");
} finally { rmSync(base,{recursive:true,force:true}); }
