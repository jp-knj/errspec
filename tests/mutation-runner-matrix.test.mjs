import {temporaryProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from "node:assert/strict";
import {test} from "node:test";
import {writeFileSync,readFileSync,rmSync,symlinkSync,cpSync} from "node:fs";
import {join,dirname,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {spawnSync} from "node:child_process";
import {runMutation,selectTests,formatReport} from "../packages/core/index.mjs";

const repo=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
const dependencies=join(repo,"tests/fixtures/runners/node_modules");
const runners=["jest","playwright","vite-plus"];
function git(root,args){
  const result=spawnSync("git",args,{cwd:root,encoding:"utf8"});
  assert.equal(result.status,0,result.stderr);return result.stdout;
}
function commit(root){
  git(root,["add","-A"]);
  git(root,["-c","commit.gpgSign=false","-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-q","-m","fixture"]);
}
function fixture(t,runner,{projects=true}={}){
  const root=temporaryProject(t,"errspec-runner-matrix-");
  const control=temporaryProject(t,"errspec-runner-matrix-control-");

  cpSync(join(repo,"tests/fixtures/runners",runner),root,{recursive:true});
  rmSync(join(root,runner==="playwright"?"value.spec.mjs":"value.test.ts"));
  symlinkSync(dependencies,join(root,"node_modules"),"dir");
  const extension=runner==="playwright"?"spec.mjs":"test.ts";
  const tracker=join(control,"executions");writeFileSync(tracker,"");
  const module=runner==="jest"?"@jest/globals":runner==="playwright"?"@errspec/playwright":"vite-plus/test";
  const header='import {test,expect'+(runner==="jest"?",jest":"")+'} from '+JSON.stringify(module)+';import {appendFileSync} from "node:fs";'+
    (runner==="playwright"?"":'import {checked} from "./src/value.ts";')+
    'const log=()=>appendFileSync('+JSON.stringify(tracker)+',"x");\n'+
    (runner==="jest"?"jest.retryTimes(2);\n":"");
  const before=runner==="playwright"?'async({page})=>{log();await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");':"async()=>{log();";
  const value=n=>runner==="playwright"?'await page.evaluate(()=>checked('+n+'))':"checked("+n+")";
  writeFileSync(join(root,"early."+extension),header+
    'test("early",'+before+'expect('+value(1)+').toBe(true)});\n'+
    'test("observes",'+before+value(1)+'});\n');
  writeFileSync(join(root,"later."+extension),header+
    'test("late",'+before+'await new Promise(r=>setTimeout(r,20));expect('+value(2)+').toBe(true)});\n'+
    'test("zero",'+before+'expect('+value(0)+').toBe(false)});\n');
  writeFileSync(join(root,"other."+extension),header+'test("unrelated",()=>{log();expect(1).toBe(1)});\n');
  writeFileSync(join(root,"src/value.ts"),"export const checked=(value:number)=>value>0;\n");
  writeFileSync(join(root,".gitignore"),"node_modules/\n.errspec/\n.cache/\ntest-results/\n");
  if(runner==="jest")writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"node",transform:{},testMatch:["**/*.test.ts"],bail:1,testTimeout:2000};\n');
  else if(runner==="playwright")writeFileSync(join(root,"playwright.config.mjs"),'export default {testDir:".",testMatch:"*.spec.mjs",maxFailures:1,retries:2,repeatEach:2,timeout:3000,use:{headless:true}'+
    (projects?',projects:[{name:"first",retries:2,repeatEach:3,use:{browserName:"chromium"}},{name:"second",retries:2,repeatEach:3,use:{browserName:"chromium"}}]':"")+"};\n");
  else writeFileSync(join(root,"vite.config.mjs"),'export default {test:{include:["*.test.ts"],bail:1,retry:2}};\n');
  git(root,["init","-q","-b","main"]);commit(root);
  return {root,tracker,extension,multiple:runner==="playwright"&&projects?2:1};
}
function cli(root,runner,command,args=[],exit=0){
  return runJsonCommand(binary,[command,"--node",process.execPath,"--runner",runner,"--include","src/value.ts","--format","json",...args],{cwd:root,encoding:"utf8",timeout:240000,killSignal:"SIGKILL"},exit);
}
const identities=report=>report.mutants.map(m=>({key:m.key,status:m.status,coveredBy:m.coveredBy,killedBy:m.killedBy,matrix:m.matrix}));
const selected=report=>Object.fromEntries(["files","tests","changedTests","covering","killers","warnings","fallbackReason","selection"].filter(k=>k in report).map(k=>[k,report[k]]));
const options=runner=>({node:process.execPath,runner,include:["src/value.ts"]});

for(const runner of runners)test(runner+" acquires every covering test across files/projects with bail/retries disabled and preserves real matrix reports",async t=>{
  const {root,multiple}=fixture(t,runner);
  const complete=cli(root,runner,"mutate",["--kill-matrix","--weak-mutation","--cache"]);
  assert.equal(complete.runComplete,true,JSON.stringify(complete.diagnostics));
  assert.equal(complete.engine.runner,runner);
  assert.equal(complete.summary.runtimeError,0,JSON.stringify(complete.mutants.map(m=>({mutator:m.mutator,status:m.status,reason:m.reason,matrix:m.matrix}))));
  assert.equal(complete.cache.reused,0);
  assert.equal(complete.killMatrix.mutants.full,complete.mutants.length,JSON.stringify(complete.mutants.map(m=>({mutator:m.mutator,status:m.status,attempts:m.attempts,matrix:m.matrix}))));
  assert.ok(complete.mutants.length>=3);
  const arrow=complete.mutants.find(m=>m.mutator==="ArrowFunction");
  assert.equal(arrow.matrix.complete,true);
  assert.equal(arrow.killedBy.length,3*multiple,"all early/late/zero killers must be acquired");
  assert.ok(arrow.killedBy.some(id=>JSON.parse(id)[1]==="later."+(runner==="playwright"?"spec.mjs":"test.ts")&&JSON.parse(id)[2]==="late"));
  for(const mutant of complete.mutants){
    assert.equal(mutant.matrix.complete,true,mutant.matrix.incompleteReason);
    assert.equal(mutant.matrix.selected.length,4*multiple);
    assert.equal(mutant.matrix.executed.length,4*multiple);
    assert.equal(new Set(mutant.matrix.executed).size,4*multiple);
    assert.ok(mutant.matrix.executed.some(id=>JSON.parse(id)[2]==="observes"));
    assert.ok(mutant.matrix.executed.every(id=>JSON.parse(id)[2]!=="unrelated"));
  }
  assert.ok(complete.mutants.some(m=>m.mutator==="EqualityOperator"&&m.killedBy.length===multiple&&m.killedBy.every(id=>JSON.parse(id)[2]==="zero")),"not-infected positive observations must still execute for matrix cells");
  assert.equal(complete.killMatrix.noKills.length,multiple);
  if(runner==="playwright"){
    assert.deepEqual([...new Set(arrow.matrix.executed.map(id=>JSON.parse(id)[0]))].sort(),["first","second"]);
    assert.equal(complete.baseline.filter(t=>t.started).length,10,"project repeats must be disabled without merging project identities");
  }
  for(const reporter of ["agent","json","html"]){
    const output=await formatReport(complete,{reporter},{binary,cwd:root});
    const report=reporter==="html"?JSON.parse(output.match(/<script type="application\/json" id="report-data">([\s\S]*?)<\/script>/)[1]):JSON.parse(output);
    assert.deepEqual(report.killMatrix,complete.killMatrix);
  }
  const api=await runMutation({...options(runner),killMatrix:true,weakMutation:true,cache:true},{binary,cwd:root});
  assert.equal(api.exitCode,0);assert.deepEqual(identities(api.report),identities(complete));
  const pending=cli(root,runner,"mutate",["--kill-matrix","--time-budget","0"]);
  assert.equal(pending.killMatrix.mutants.full,0);assert.ok(pending.killMatrix.mutants.incomplete>0);
  assert.ok(pending.killMatrix.rows.every(row=>!row.complete&&row.executed.length===0));
  assert.deepEqual(pending.killMatrix.noKills,[]);
});

for(const runner of runners)test(runner+" selectTests uses current Git coverage without trials and never selects a deleted killer",async t=>{
  const {root,tracker,extension,multiple}=fixture(t,runner);
  writeFileSync(join(root,"src/value.ts"),"export const checked=(value:number)=>/* changed */value>0;\n");
  const before=readFileSync(tracker,"utf8").length;
  const report=cli(root,runner,"select-tests",["--since","HEAD"]);
  assert.equal(report.runComplete,true,JSON.stringify(report.diagnostics));
  assert.deepEqual(report.files,["early."+extension,"later."+extension]);
  assert.equal(readFileSync(tracker,"utf8").length-before,10*multiple,"only the plain/probe cohort runs");
  const api=await selectTests({...options(runner),since:"HEAD"},{binary,cwd:root});
  assert.equal(api.exitCode,0);assert.deepEqual(selected(api.report),selected(report));
  const files=spawnSync(binary,["select-tests","--node",process.execPath,"--runner",runner,"--include","src/value.ts","--since","HEAD","--format","files"],{cwd:root,encoding:"utf8",timeout:120000});
  assert.equal(files.status,0,files.stderr);assert.equal(files.stdout,report.files.join("\n")+"\n");

  rmSync(join(root,"early."+extension));
  const removed=cli(root,runner,"select-tests",["--since","HEAD"]);
  assert.ok(!removed.files.includes("early."+extension));
  assert.ok(![...removed.covering,...removed.killers].some(t=>JSON.parse(t.test)[1]==="early."+extension));
  assert.ok(removed.files.includes("other."+extension),"changed/deleted test files require the current conservative cohort");
  assert.ok(removed.warnings.some(w=>w.code==="EC_MUTATION_CHANGED_TEST_UNCOLLECTED"&&w.file==="early."+extension));
  commit(root);
  writeFileSync(join(root,"src/value.ts"),readFileSync(join(root,"src/value.ts"),"utf8")+"export const unobserved=()=>7;\n");
  const unobserved=cli(root,runner,"select-tests",["--since","HEAD"]);
  assert.deepEqual(unobserved.files,["later."+extension,"other."+extension]);
  assert.match(unobserved.fallbackReason,/no attributable|outside/);
  commit(root);
  writeFileSync(join(root,"src/value.ts"),"export const checked=(value:number)=>/* changed */value>0;\n");
  const deleted=cli(root,runner,"select-tests",["--since","HEAD"]);
  assert.deepEqual(deleted.files,["later."+extension,"other."+extension]);
  assert.match(deleted.fallbackReason,/Removed source/);
});

for(const runner of runners)test(runner+" actual per-test retries/repeats and deadlines leave matrix rows incomplete",t=>{
  const {root,extension}=fixture(t,runner,{projects:false});
  rmSync(join(root,"early."+extension));rmSync(join(root,"later."+extension));rmSync(join(root,"other."+extension));
  writeFileSync(join(root,"src/value.ts"),"export const checked=()=>true;\n");
  if(runner==="jest")writeFileSync(join(root,"retry."+extension),'import {test,expect,beforeAll} from "@jest/globals";import {checked} from "./src/value.ts";beforeAll(()=>{if(checked()!==true)globalThis[Symbol.for("RETRY_TIMES")]=1});test("retried",()=>expect(checked()).toBe(true));\n');
  else if(runner==="playwright")writeFileSync(join(root,"retry."+extension),'import {test,expect} from "@errspec/playwright";test.describe.configure({retries:1});test("retried",async({page})=>{await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");expect(await page.evaluate(()=>checked())).toBe(true)});\n');
  else writeFileSync(join(root,"retry."+extension),'import {test,expect} from "vite-plus/test";import {checked} from "./src/value.ts";test("repeated",{repeats:1},()=>expect(checked()).toBe(true));\n');
  const retried=cli(root,runner,"mutate",["--kill-matrix"]);
  assert.equal(retried.runComplete,true,JSON.stringify(retried.diagnostics));
  assert.ok(retried.killMatrix.rows.length>0);
  assert.ok(retried.killMatrix.rows.every(row=>!row.complete&&/retried|repeated/.test(row.incompleteReason)),JSON.stringify(retried.killMatrix));
  assert.equal(retried.killMatrix.mutants.full,0);
  assert.deepEqual(retried.killMatrix.noKills,[]);

  if(runner==="playwright")writeFileSync(join(root,"retry."+extension),'import {test,expect} from "@errspec/playwright";test("deadline",async({page})=>{test.setTimeout(1500);await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");const value=await page.evaluate(()=>checked());if(value!==true)await new Promise(()=>{});expect(value).toBe(true)});\n');
  else writeFileSync(join(root,"retry."+extension),'import {test,expect} from '+JSON.stringify(runner==="jest"?"@jest/globals":"vite-plus/test")+';import {checked} from "./src/value.ts";test("deadline",async()=>{const value=checked();if(value!==true)await new Promise(()=>{});expect(value).toBe(true)},40);\n');
  const timeout=cli(root,runner,"mutate",["--kill-matrix"]);
  assert.ok(timeout.killMatrix.mutants.timeout>0,JSON.stringify(timeout.killMatrix));
  assert.ok(timeout.killMatrix.rows.filter(row=>row.status==="Timeout").every(row=>!row.complete&&row.incompleteReason));
});
