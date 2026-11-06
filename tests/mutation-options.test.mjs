import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, symlinkSync, realpathSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import ts from "../packages/analysis-driver/node_modules/typescript6/lib/typescript.js";
import { defineConfig, runMutation } from "../packages/core/index.mjs";

const repo=fileURLToPath(new URL("../",import.meta.url)), binary=join(repo,"target/debug/errspec");
const sdk=pathToFileURL(join(repo,"packages/core/index.mjs")).href;
function project(t) {
  const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-options-")));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"src/value.ts"),"export const yes=()=>true;");
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {yes} from "./src/value";test("yes",()=>expect(yes()).toBeDefined());');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"]}};');
  return root;
}
function cli(root,args=[],code=0) {
  const result=spawnSync(binary,["mutate","--node",process.execPath,"--format","json",...args],{cwd:root,encoding:"utf8",timeout:30000});
  assert.ifError(result.error);assert.equal(result.status,code,result.stdout+result.stderr);
  return JSON.parse(result.stdout);
}
test("all five configuration formats use the same native validation and keep logs out of JSON",t=>{
  const root=project(t);
  for(const ext of ["json","js","mjs","mts","ts"]) {
    const path=join(root,`settings.${ext}`);
    const options=defineConfig({weakMutation:true,policy:{timeBudget:0}});
    writeFileSync(path,ext==="json"?JSON.stringify(options):`import {defineConfig} from ${JSON.stringify(sdk)}; console.log('config log'); export default defineConfig(${JSON.stringify(options)});`);
    const result=cli(root,["--config-file",path]);
    assert.equal(result.policy.options.timeBudget,0);
    assert.ok(result.mutants.some(m=>m.status==="Pending"));
  }
});
test("explicit flags and negative flags override the file while omitted values keep it",t=>{
  const root=project(t),path=join(root,"errspec.mutation.config.json");
  writeFileSync(path,JSON.stringify({typecheck:true,typecheckProject:"missing.json",weakMutation:true,failOnUndetected:true,policy:{timeBudget:0,failOnSurvived:true,thresholdHigh:90,thresholdLow:30}}));
  const result=cli(root,["--no-typecheck","--no-weak-mutation","--no-fail-on-undetected","--no-fail-on-survived"]);
  assert.equal(result.engine.typecheck,"not_run");
  assert.equal(result.gate.enabled,false);
  assert.equal(result.policy.options.failOnSurvived,false);
  assert.equal(result.policy.options.thresholdHigh,90);
  assert.equal(result.policy.options.thresholdLow,30);
});
test("unknown keys, wrong types, invalid policies and missing exports stop before tests",t=>{
  const root=project(t);
  for(const data of [{unknown:true},{weakMutation:"true"},{include:[1]},{policy:{timeBudget:-1}},{policy:{unknown:true}},{typecheck:true,typecheckProject:42}]) {
    writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify(data));
    const result=cli(root,["--no-typecheck"],2);
    assert.equal(result.testsExecuted,false);
    assert.equal(result.diagnostics[0].code,"EC_MUTATION_CONFIG");
  }
  rmSync(join(root,"errspec.mutation.config.json"));
  for(const source of ["export default []", "export default {get weakMutation(){throw Error('getter')}}", "export default ()=>({})", "export default {weakMutation:()=>true}", "export const options={}"]) {
    writeFileSync(join(root,"errspec.mutation.config.mjs"),source);
    assert.equal(cli(root,[],2).testsExecuted,false);
  }
});
test("ambiguous configs and missing explicitly selected files cannot fall back to defaults",t=>{
  const root=project(t);
  writeFileSync(join(root,"errspec.mutation.config.json"),"{}");
  writeFileSync(join(root,"errspec.mutation.config.mjs"),"export default {};");
  assert.match(cli(root,[],2).diagnostics[0].message,/Multiple/);
  assert.equal(cli(root,["--config-file","missing.json"],2).testsExecuted,false);
});
test("programmatic execution shares config merging, quality verdicts and candidate identities",async t=>{
  const root=project(t),path=join(root,"errspec.mutation.config.json");
  writeFileSync(path,JSON.stringify({policy:{thresholdHigh:90,thresholdLow:10,timeBudget:0}}));
  const api=await runMutation({node:process.execPath,policy:{timeBudget:0}},{binary,cwd:root});
  const command=cli(root);
  assert.equal(api.exitCode,0);
  // Timings are observations from distinct executions, not part of their verdict.
  const verdict=report=>({...report,baseline:report.baseline.map(({duration,...test})=>test)});
  assert.deepEqual(verdict(api.report),verdict(command));
  assert.equal(api.report.policy.options.thresholdLow,10);
  const failed=await runMutation({node:process.execPath,policy:{critical:["src/**"]}},{binary,cwd:root});
  assert.equal(failed.exitCode,1);
  assert.equal(failed.report.runComplete,true);
  const bad=await runMutation({unknown:true},{binary,cwd:root});
  assert.equal(bad.exitCode,2);assert.equal(bad.report.testsExecuted,false);
  await assert.rejects(runMutation({typecheck:()=>true},{binary,cwd:root}),/JSON values/);
  const schema=await runMutation(defineConfig({$schema:"./schema.json",node:process.execPath,policy:{timeBudget:0}}),{binary,cwd:root});
  assert.equal(schema.exitCode,0);
  const badSchema=await runMutation({$schema:42},{binary,cwd:root});
  assert.equal(badSchema.exitCode,2);
});
test("configured paths use the project root; explicit CLI paths keep invoking-directory semantics",async t=>{
  const root=project(t),outer=realpathSync(mkdtempSync(join(tmpdir(),"errspec-config-parent-")));
  t.after(()=>rmSync(outer,{recursive:true,force:true}));
  symlinkSync(root,join(outer,"project"),"dir");
  writeFileSync(join(outer,"settings.json"),JSON.stringify({root:"project",vitestConfig:"vitest.config.mjs",policy:{timeBudget:0}}));
  const api=await runMutation({node:process.execPath},{binary,cwd:outer,configFile:"settings.json"});
  assert.equal(api.exitCode,0,JSON.stringify(api));
  writeFileSync(join(outer,"explicit.mjs"),'export default {test:{include:["value.test.ts"]}};');
  const explicit=cli(outer,["--config-file","settings.json","--root",root,"--vitest-config","explicit.mjs"]);
  assert.equal(explicit.runComplete,true);
  const original=readFileSync(join(root,"src/value.ts"),"utf8");
  assert.equal(original,"export const yes=()=>true;");
});
test("SDK reports missing binaries and cancellation without claiming successful execution",async()=>{
  await assert.rejects(runMutation({}, {binary:"/does-not-exist"}),/ENOENT/);
  const signal=AbortSignal.abort(new Error("cancelled"));
  await assert.rejects(runMutation({}, {binary,signal}),/cancelled/);
});
test("configuration loaders bound output and time, and cancellation preserves its exit code",{timeout:25000},async t=>{
  const root=project(t),config=join(root,"errspec.mutation.config.mjs");
  writeFileSync(config,"console.log('x'.repeat(1100000));export default {};");
  assert.match(cli(root,[],2).diagnostics[0].message,/exceeds 1 MiB/);
  writeFileSync(config,"await new Promise(()=>setInterval(()=>{},1000));export default {};");
  assert.match(cli(root,[],2).diagnostics[0].message,/timed out/);
  writeFileSync(config,"import {writeFileSync} from 'node:fs';writeFileSync(new URL('./ready',import.meta.url),'yes');await new Promise(()=>setInterval(()=>{},1000));export default {};");
  const controller=new AbortController();
  const pending=runMutation({node:process.execPath},{binary,cwd:root,signal:controller.signal});
  const deadline=Date.now()+5000;
  while(!existsSync(join(root,"ready"))&&Date.now()<deadline)await delay(20);
  assert.ok(existsSync(join(root,"ready")),"Configuration loader did not start");
  controller.abort();
  const result=await pending;
  assert.equal(result.exitCode,130);
  assert.equal(result.report.runComplete,false);
  assert.equal(result.report.testsExecuted,false);
});
test("published API declarations support strict TypeScript consumers",t=>{
  const root=project(t),file=join(root,"consumer.mts");
  writeFileSync(file,`import {defineConfig,runMutation,formatReport} from ${JSON.stringify(join(repo,"packages/core/index.mjs"))};
const config=defineConfig({typecheck:false,weakMutation:true,vitestConfig:null,policy:{timeBudget:0}});
const result=await runMutation(config);
const complete:boolean=result.report.runComplete;
const rendered:string=await formatReport(result.report,{reporter:'agent',sources:{'src/value.ts':'export const value=true;'}});
// @ts-expect-error unsupported reporter
formatReport(result.report,{reporter:'markdown'});
const key:string|undefined=result.report.mutants[0]?.key;
const offset:number|undefined=result.report.mutants[0]?.byteOffset;
// @ts-expect-error unknown option
defineConfig({typo:true});
// @ts-expect-error incorrect option type
runMutation({weakMutation:'true'});
// @ts-expect-error report contains numeric counts
const count:string=result.report.summary.killed;
void [complete,key,offset,count,rendered];`);
  const program=ts.createProgram([file],{strict:true,noUncheckedIndexedAccess:true,exactOptionalPropertyTypes:true,noEmit:true,skipLibCheck:false,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.NodeNext,moduleResolution:ts.ModuleResolutionKind.NodeNext});
  const diagnostics=ts.getPreEmitDiagnostics(program);
  assert.deepEqual(diagnostics.map(d=>ts.flattenDiagnosticMessageText(d.messageText,"\n")),[]);
});
test("finished config loaders terminate their owned descendants",t=>{
  const root=project(t),marker=join(root,"descendant-lived");
  writeFileSync(join(root,"errspec.mutation.config.mjs"),`
import {spawn} from 'node:child_process';
spawn(process.execPath,['-e',${JSON.stringify(`setTimeout(()=>require('node:fs').writeFileSync(${JSON.stringify(marker)},'alive'),400);setInterval(()=>{},1000);`)}],{stdio:'ignore'}).unref();
export default {policy:{timeBudget:0}};
`);
  assert.equal(cli(root).runComplete,true);
  assert.equal(existsSync(marker),false,"A loader descendant survived after its owned group was closed");
});
