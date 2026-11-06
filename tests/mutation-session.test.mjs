import {temporaryProject,prepareVitestProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import {mkdirSync,writeFileSync,readFileSync,rmSync} from "node:fs";
import {join} from "node:path";
import {fileURLToPath,pathToFileURL} from "node:url";
const repo=fileURLToPath(new URL("../",import.meta.url)),binary=join(repo,"target/debug/errspec");
function fixture(t,globalSetup=false){
  const root=temporaryProject(t,"errspec-session-",{canonical:false});
  prepareVitestProject(root,repo);

  writeFileSync(join(root,"src/value.ts"),"export const add=(a:number,b:number)=>a+b;\nexport const flag=()=>true;");
  writeFileSync(join(root,"a.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";globalThis.__fixtureRuns=(globalThis.__fixtureRuns||0)+1;test("addition",()=>{expect(globalThis.__fixtureRuns).toBe(1);expect(Math.abs(-2)).toBe(2);expect(add(2,3)).toBe(5)});');
  writeFileSync(join(root,"b.test.ts"),'import {test,expect,vi} from "vitest";import {flag} from "./src/value";globalThis.__fixtureRuns=(globalThis.__fixtureRuns||0)+1;test("weak flag",()=>{expect(globalThis.__fixtureRuns).toBe(1);vi.spyOn(Math,"abs").mockReturnValue(999);expect(flag()).toBeDefined()});');
  writeFileSync(join(root,"vitest.config.mjs"),`import {appendFileSync} from 'node:fs';export default {plugins:[{name:'count-context',configResolved(){appendFileSync(${JSON.stringify(join(root,"contexts"))},'created\\n')}}],test:{${globalSetup?"globalSetup:['./global.mjs']":""}}};`);
  if(globalSetup)writeFileSync(join(root,"global.mjs"),`import {appendFileSync} from 'node:fs';export default ()=>{appendFileSync(${JSON.stringify(join(root,"setups"))},'setup\\n');return ()=>appendFileSync(${JSON.stringify(join(root,"teardowns"))},'teardown\\n')};`);
  return root;
}
function run(root,args=[],code=0){
  return runJsonCommand(binary,["mutate","--node",process.execPath,"--format","json",...args],{cwd:root,encoding:"utf8",timeout:60000},code);
}
const verdict=r=>r.mutants.map(m=>[m.key,m.status,m.coveredBy,m.killedBy,m.attempts]);
function runnerProxy(root,decorate){
  const directory=join(root,"runner"),dist=join(directory,"dist"),vitest=join(repo,"examples/checkout/node_modules/vitest");
  mkdirSync(dist,{recursive:true});
  writeFileSync(join(directory,"package.json"),JSON.stringify({name:"vitest",version:"5.0.5",type:"module"}));
  writeFileSync(join(dist,"index.js"),`export * from ${JSON.stringify(pathToFileURL(join(vitest,"dist/index.js")).href)};`);
  writeFileSync(join(dist,"node.js"),`import {appendFileSync} from 'node:fs';import {createVitest as actual} from ${JSON.stringify(pathToFileURL(join(vitest,"dist/node.js")).href)};export async function createVitest(...args){const ctx=await actual(...args);${decorate};return ctx;}`);
  return directory;
}
test("session reuse preserves survivors, globals, mocks, request identities and per-test selection",{timeout:60000},t=>{
  const root=fixture(t),fresh=run(root),freshCount=readFileSync(join(root,"contexts"),"utf8").length;
  writeFileSync(join(root,"contexts"),"");
  const reused=run(root,["--reuse-session"]),warmCount=readFileSync(join(root,"contexts"),"utf8").length;
  assert.equal(reused.engine.sessionReuse,"available");
  assert.deepEqual(verdict(reused),verdict(fresh));assert.ok(reused.summary.survived>0);
  assert.ok(warmCount<freshCount,`Expected fewer context creations: ${warmCount} vs ${freshCount}`);
  assert.equal(readFileSync(join(root,"src/value.ts"),"utf8"),"export const add=(a:number,b:number)=>a+b;\nexport const flag=()=>true;");
});
test("global setup retains fresh sessions with balanced teardown",{timeout:60000},t=>{
  const root=fixture(t,true),result=run(root,["--reuse-session"]);
  assert.equal(result.engine.sessionReuse,"fresh_global_setup");
  assert.equal(readFileSync(join(root,"setups"),"utf8").replaceAll('setup',''),readFileSync(join(root,"teardowns"),"utf8").replaceAll('teardown',''));
  assert.ok(result.summary.survived>0);
});
test("configuration and negative flags select reuse explicitly",t=>{
  const root=fixture(t);
  writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify({reuseSession:true,policy:{timeBudget:0}}));
  assert.equal(run(root).engine.sessionReuse,"available");
  assert.equal(run(root,["--no-reuse-session"]).engine.sessionReuse,undefined);
});
test("timeout retries extend only that attempt and restore the next candidate's timeouts",{timeout:60000},t=>{
  const root=fixture(t),log=join(root,"timeouts");
  writeFileSync(join(root,"src/value.ts"),"export const a=()=>true;\nexport const b=()=>true;");
  rmSync(join(root,"b.test.ts"));
  writeFileSync(join(root,"vitest.config.mjs"),"export default {test:{testTimeout:100,hookTimeout:1000}};");
  writeFileSync(join(root,"a.test.ts"),`import {test,expect} from 'vitest';import {a,b} from './src/value';test('timeout isolation',async()=>{if(a()!==true)await new Promise(resolve=>setTimeout(resolve,2000));expect(b()).toBeDefined();});`);
  const proxy=runnerProxy(root,`const run=ctx.runTestSpecifications.bind(ctx);ctx.runTestSpecifications=(...specs)=>{const config=ctx.projects[0].config;appendFileSync(${JSON.stringify(log)},JSON.stringify({key:config.provide.__errspecMutation.activeKey,timeout:config.testTimeout,hook:config.hookTimeout})+'\\n');return run(...specs)}`);
  const report=run(root,["--reuse-session","--vitest-package",proxy]),observed=readFileSync(log,"utf8").trim().split('\n').map(line=>JSON.parse(line));
  assert.equal(report.engine.sessionReuse,"available");assert.ok(report.summary.timeout>0);assert.ok(report.summary.survived>0);
  for(const mutant of report.mutants){
    const values=observed.filter(row=>row.key===mutant.key);
    assert.deepEqual(values.map(row=>row.timeout),mutant.status==="Timeout"?[100,300]:[100]);
    assert.deepEqual(values.map(row=>row.hook),mutant.status==="Timeout"?[1000,3000]:[1000]);
  }
});
test("stale runner events invalidate a reused session instead of detecting a different mutant",t=>{
  const root=fixture(t),proxy=runnerProxy(root,`let runs=0;const run=ctx.runTestSpecifications.bind(ctx);ctx.runTestSpecifications=async(...specs)=>{const result=await run(...specs);if(++runs===2){const module=result.testModules[0];module.meta().__errspecMutation.requestId--;args[0].reporters[0].onTestModuleEnd(module);}return result}`);
  const report=run(root,["--reuse-session","--vitest-package",proxy],2);
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_PROTOCOL");
  assert.match(report.diagnostics[0].message,/Stale (test|module) metadata/);
});
test("missing lifecycle APIs retain the fresh-session execution path",t=>{
  const root=fixture(t),proxy=runnerProxy(root,"ctx.clearSpecificationsCache=undefined");
  const report=run(root,["--reuse-session","--vitest-package",proxy]);
  assert.equal(report.engine.sessionReuse,"fresh_lifecycle_unknown");
  assert.ok(report.summary.survived>0);
});
test("cleanup failures cannot become successful mutation reports",t=>{
  const root=fixture(t,true);
  writeFileSync(join(root,"global.mjs"),"export default ()=>()=>{throw new Error('cleanup broke')};");
  const report=run(root,["--reuse-session"],2);
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_RUNNER");
  assert.match(report.diagnostics[0].message,/cleanup reported a failure/);
});
