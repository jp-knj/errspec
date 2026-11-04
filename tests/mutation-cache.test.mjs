import assert from "node:assert/strict";
import test from "node:test";
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath,pathToFileURL} from "node:url";
import {runMutation} from "../packages/core/index.mjs";
const repo=fileURLToPath(new URL("../",import.meta.url)),binary=join(repo,"target/debug/errspec");
function fixture(t){
  const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-cache-")));t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"module",dependencies:{vitest:"5.0.5"}}));
  writeFileSync(join(root,"src/value.ts"),"export const add=(a:number,b:number)=>a+b;");
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test("adds",()=>expect(add(2,3)).toBe(5));');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"]}};');
  return root;
}
function run(root,args=[],code=0,environment={}){
  const result=spawnSync(binary,["mutate","--node",process.execPath,"--format","json",...args],{cwd:root,encoding:"utf8",timeout:60000,env:{...process.env,...environment}});
  assert.ifError(result.error);assert.equal(result.status,code,result.stdout+result.stderr);return JSON.parse(result.stdout);
}
test("cache is opt-in, preserves current baselines, and labels reused assertion outcomes",{timeout:60000},t=>{
  const root=fixture(t),off=run(root);
  assert.equal(off.cache,undefined);assert.ok(off.mutants.every(m=>m.reused===undefined));
  const cold=run(root,["--cache"]),warm=run(root,["--cache"]);
  assert.equal(cold.cache.disabledReason,undefined,JSON.stringify(cold.cache));
  assert.equal(cold.cache.executed,cold.summary.killed);assert.equal(cold.cache.reused,0);assert.ok(cold.cache.saved>0);
  assert.equal(warm.cache.disabledReason,undefined,JSON.stringify(warm.cache));
  assert.equal(warm.cache.executed,0);assert.equal(warm.cache.reused,cold.summary.killed);
  assert.equal(warm.cache.coverageReused,true);
  assert.equal(warm.testsExecuted,true);assert.equal(warm.baseline.length,1);
  assert.ok(warm.mutants.every(m=>m.reused===true));
  assert.deepEqual(warm.mutants.map(m=>[m.key,m.status,m.killedBy]),cold.mutants.map(m=>[m.key,m.status,m.killedBy]));
});
test("coverage reuse skips only the probe and full dry runs refresh it independently",{timeout:60000},t=>{
  const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),"errspec-probe-calls-"))),calls=join(control,"calls");
  t.after(()=>rmSync(control,{recursive:true,force:true}));writeFileSync(calls,"");
  writeFileSync(join(root,"value.test.ts"),`import {appendFileSync} from 'node:fs';import {test,expect} from 'vitest';import {add} from './src/value';test('adds',()=>{appendFileSync(${JSON.stringify(calls)},'x');expect(add(2,3)).toBe(5)});`);
  const cold=run(root,["--cache"]);assert.equal(cold.cache.coverageReused,undefined);
  const count=()=>readFileSync(calls,"utf8").length,before=count();
  const warm=run(root,["--cache"]);assert.equal(warm.cache.coverageReused,true);assert.equal(count()-before,1);
  assert.equal(warm.cache.executed,0);assert.equal(warm.testsExecuted,true);
  const after=count(),refresh=run(root,["--cache","--full-dry-run"]);
  assert.equal(refresh.cache.coverageReused,undefined);assert.equal(refresh.cache.reused,cold.summary.killed);assert.equal(count()-after,2);
  writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify({cache:true,fullDryRun:true}));
  run(root);assert.equal(run(root,["--no-full-dry-run"]).cache.coverageReused,true);
});
test("corrupt coverage refreshes observations and failed current baselines cannot use old evidence",{timeout:60000},t=>{
  const root=fixture(t);run(root,["--cache"]);
  writeFileSync(join(root,".errspec/mutation-cache/coverage-v1.json"),"{bad");
  const report=run(root,["--cache"]);assert.equal(report.cache.coverageReused,undefined);
  assert.ok(report.cache.warnings.some(w=>w.startsWith("Coverage cache")));assert.ok(report.summary.killed>0);
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";test("broken",()=>expect(1).toBe(2));');
  const failed=run(root,["--cache"],2);assert.equal(failed.runComplete,false);assert.equal(failed.summary.score,null);
  assert.equal(failed.cache.reused,0);assert.equal(failed.cache.coverageReused,undefined);
});
test("cached probe observations preserve weak infection and static/concurrent attribution",{timeout:60000},t=>{
  for(const mode of ["weak","static-concurrent"]){
    const root=fixture(t);
    if(mode==="weak"){
      writeFileSync(join(root,"src/value.ts"),'export const add=(a:number,b:number)=>a+b;');
      writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test("adds",()=>expect(add(2,0)).toBe(2));');
    }else{
      writeFileSync(join(root,"src/value.ts"),'const base=true?1:0;export const add=(a:number,b:number)=>a+b+base;');
      writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test.concurrent("adds",()=>expect(add(2,3)).toBe(6));test.concurrent("adds again",()=>expect(add(1,2)).toBe(4));');
    }
    const args=["--cache",...(mode==="weak"?["--weak-mutation"]:[])],cold=run(root,args),warm=run(root,args);
    assert.equal(warm.cache.coverageReused,true);assert.equal(warm.cache.executed,0);
    assert.deepEqual(warm.mutants.map(m=>[m.key,m.status,m.coveredBy,m.infectedBy]),cold.mutants.map(m=>[m.key,m.status,m.coveredBy,m.infectedBy]));
    if(mode==="weak")assert.ok(warm.mutants.some(m=>m.reason?.startsWith("not infected")));
  }
});
for(const major of [5,6,7])test(`TS ${major} typechecking remains current with cached probe and trial results`,{timeout:60000},t=>{
  const root=fixture(t),source='export const add=(a:number,b:number):number=>a+b;';
  writeFileSync(join(root,"src/value.ts"),source);
  writeFileSync(join(root,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,noUncheckedIndexedAccess:true,target:"ES2022",module:"ESNext",moduleResolution:"Bundler",skipLibCheck:true},files:["src/value.ts"]}));
  const args=["--cache","--typecheck","--typecheck-package",join(repo,`packages/analysis-driver/node_modules/typescript${major}`)];
  const cold=run(root,args),warm=run(root,args);
  assert.equal(warm.cache.coverageReused,true);assert.equal(warm.cache.executed,0);assert.equal(warm.cache.reused,cold.summary.killed);
  assert.ok(warm.typecheck.checked>0);assert.ok(warm.mutants.some(m=>m.status==="CompileError"));
  assert.deepEqual(warm.mutants.map(m=>[m.key,m.status]),cold.mutants.map(m=>[m.key,m.status]));
  assert.equal(readFileSync(join(root,"src/value.ts"),"utf8"),source);
});
test("dependency contents, configuration and environment changes invalidate stored trials",{timeout:60000},t=>{
  const root=fixture(t),dependency=join(root,"node_modules/local-value");mkdirSync(dependency);
  writeFileSync(join(dependency,"package.json"),JSON.stringify({name:"local-value",version:"1.0.0",type:"module",exports:"./index.js"}));
  writeFileSync(join(dependency,"index.js"),"export const delta=1;");
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"module",dependencies:{vitest:"5.0.5","local-value":"1.0.0"}}));
  writeFileSync(join(root,"src/value.ts"),'import {delta} from "local-value";export const add=(a:number,b:number)=>a+b+delta;');
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test("adds",()=>expect(add(2,3)).toBe(6));');
  run(root,["--cache"]);
  writeFileSync(join(dependency,"index.js"),"export const delta=1; // dependency changed\n");
  const dependencyChange=run(root,["--cache"]);assert.equal(dependencyChange.cache.reused,0);assert.equal(dependencyChange.cache.invalidated,true);
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"],testTimeout:5000}};');
  const config=run(root,["--cache"]);assert.equal(config.cache.reused,0);assert.equal(config.cache.invalidated,true);
  const environment=run(root,["--cache"],0,{ERRSPEC_CACHE_INPUT_TEST:"changed"});
  assert.equal(environment.cache.reused,0);assert.equal(environment.cache.invalidated,true);
});
test("removing a killing assertion produces current uncovered results",{timeout:60000},t=>{
  const root=fixture(t);const previous=run(root,["--cache"]);assert.ok(previous.summary.killed>0);
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";test("smoke",()=>expect(1).toBe(1));');
  const current=run(root,["--cache"]);
  assert.equal(current.cache.reused,0);assert.equal(current.summary.killed,0);assert.ok(current.summary.noCoverage>0);
});
test("resolved provided values invalidate results even when configuration source is unchanged",{timeout:60000},t=>{
  const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),"errspec-cache-provide-"))),value=join(control,"value");
  t.after(()=>rmSync(control,{recursive:true,force:true}));writeFileSync(value,"first");
  writeFileSync(join(root,"vitest.config.mjs"),`import {readFileSync} from 'node:fs';export default {test:{include:['value.test.ts'],provide:{value:readFileSync(${JSON.stringify(value)},'utf8')}}};`);
  const cold=run(root,["--cache"]);assert.ok(cold.cache.saved>0);
  assert.equal(run(root,["--cache"]).cache.reused,cold.summary.killed);
  writeFileSync(value,"second");
  const changed=run(root,["--cache"]);assert.equal(changed.cache.reused,0);assert.equal(changed.cache.invalidated,true);
});
test("resolved Vite defines invalidate cached detections when configuration source is unchanged",{timeout:60000},t=>{
  const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-cache-define-'))),value=join(control,'value');
  t.after(()=>rmSync(control,{recursive:true,force:true}));writeFileSync(value,'1');
  writeFileSync(join(root,'src/value.ts'),'export const add=(value:number)=>value-value;');
  writeFileSync(join(root,'value.test.ts'),`import {test,expect} from 'vitest';import {add} from './src/value';test('cancellation',()=>expect(add(__ERRSPEC_INPUT__)).toBe(0));`);
  writeFileSync(join(root,'vitest.config.mjs'),`import {readFileSync} from 'node:fs';export default {define:{__ERRSPEC_INPUT__:readFileSync(${JSON.stringify(value)},'utf8')},test:{include:['value.test.ts']}};`);
  const cold=run(root,['--cache']);assert.equal(cold.summary.killed,2);
  assert.equal(run(root,['--cache']).cache.reused,2);
  writeFileSync(value,'0');
  const changed=run(root,['--cache']);assert.equal(changed.cache.reused,0);assert.equal(changed.cache.invalidated,true);assert.equal(changed.cache.coverageReused,undefined);assert.equal(changed.summary.survived,1);
});
test("environment compilation overrides participate in the cache fingerprint",{timeout:60000},t=>{
  const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-cache-environment-'))),value=join(control,'value');
  t.after(()=>rmSync(control,{recursive:true,force:true}));writeFileSync(value,'1');
  writeFileSync(join(root,'src/value.ts'),'export const add=(value:number)=>value-value;');
  writeFileSync(join(root,'value.test.ts'),`import {test,expect} from 'vitest';import {add} from './src/value';test('cancellation',()=>expect(add(__ERRSPEC_INPUT__)).toBe(0));`);
  writeFileSync(join(root,'vitest.config.mjs'),`import {readFileSync} from 'node:fs';export default {define:{__ERRSPEC_INPUT__:'0'},plugins:[{name:'fixture-environment',configEnvironment(name){if(name==='ssr'||name==='__vitest__')return {define:{__ERRSPEC_INPUT__:readFileSync(${JSON.stringify(value)},'utf8')}};}}],test:{include:['value.test.ts']}};`);
  const cold=run(root,['--cache']);assert.equal(cold.summary.killed,2);assert.equal(run(root,['--cache']).cache.reused,2);
  writeFileSync(value,'0');
  const changed=run(root,['--cache']);assert.equal(changed.cache.reused,0);assert.equal(changed.cache.invalidated,true);assert.equal(changed.summary.survived,1);
});
test("unserializable compilation options keep verification fresh",{timeout:60000},t=>{
  for(const config of [
    `resolve:{alias:[{find:'__unused_alias__',replacement:'unused',customResolver:()=>null}]},test:{include:['value.test.ts']}`,
    `define:{__UNUSED_DEFINE__:'1+0'},test:{include:['value.test.ts']}`,
    `plugins:[{name:'asset-callback',configResolved(config){config.assetsInclude=()=>false;}}],test:{include:['value.test.ts']}`,
    `test:{include:['value.test.ts'],css:{include:[/.*/]}}`,
  ]){
    const root=fixture(t);writeFileSync(join(root,'vitest.config.mjs'),`export default {${config}};`);
    for(let attempt=0;attempt<2;attempt++){
      const report=run(root,['--cache']);assert.equal(report.cache.reused,0);assert.equal(report.cache.saved,0);assert.ok(report.cache.executed>0);assert.match(report.cache.disabledReason,/configuration cannot be fingerprinted/);
    }
  }
});
test("unacquired transform closures and duplicate builtin names cannot reuse detections",{timeout:60000},t=>{
  for(const name of ['fixture-transform','alias']){
    const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-cache-transform-'))),value=join(control,'value');
    t.after(()=>rmSync(control,{recursive:true,force:true}));writeFileSync(value,'1');
    writeFileSync(join(root,'src/value.ts'),'export const add=(value:number)=>value-value;');
    writeFileSync(join(root,'value.test.ts'),`import {test,expect} from 'vitest';import {add} from './src/value';test('cancellation',()=>expect(add(__ERRSPEC_INPUT__)).toBe(0));`);
    writeFileSync(join(root,'vitest.config.mjs'),`import {readFileSync} from 'node:fs';export default {plugins:[{name:${JSON.stringify(name)},enforce:'post',transform(code,id){if(id.includes('value.test.ts'))return code.replace('__ERRSPEC_INPUT__',readFileSync(${JSON.stringify(value)},'utf8'));}}],test:{include:['value.test.ts']}};`);
    const cold=run(root,['--cache']);assert.equal(cold.summary.killed,2);assert.equal(cold.cache.saved,0);assert.match(cold.cache.disabledReason,/configuration cannot be fingerprinted/);
    writeFileSync(value,'0');const current=run(root,['--cache']);assert.equal(current.cache.reused,0);assert.equal(current.summary.survived,1);
  }
});
test("global setup inputs keep execution fresh until their lifecycle can be acquired",{timeout:60000},t=>{
  const root=fixture(t);
  writeFileSync(join(root,"setup.mjs"),'export default ({provide})=>{provide("external",Date.now());};');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"],globalSetup:["./setup.mjs"]}};');
  const report=run(root,["--cache"]);assert.equal(report.cache.reused,0);assert.equal(report.cache.saved,0);
  assert.ok(report.cache.executed>0);assert.match(report.cache.disabledReason,/configuration cannot be fingerprinted/);
});
test("changed tests and source positions conservatively revalidate rather than reuse",{timeout:60000},t=>{
  const root=fixture(t),first=run(root,["--cache"]);
  writeFileSync(join(root,"value.test.ts"),readFileSync(join(root,"value.test.ts"),"utf8")+"\n// test changed\n");
  const changed=run(root,["--cache"]);assert.equal(changed.cache.reused,0);assert.equal(changed.cache.invalidated,true);
  writeFileSync(join(root,"src/value.ts"),"\n\n"+readFileSync(join(root,"src/value.ts"),"utf8"));
  const moved=run(root,["--cache"]);assert.equal(moved.cache.reused,0);
  assert.deepEqual(moved.mutants.map(m=>m.key),first.mutants.map(m=>m.key));
});
test("corrupt caches are revalidated and cannot become detections",{timeout:60000},t=>{
  const root=fixture(t);run(root,["--cache"]);
  writeFileSync(join(root,".errspec/mutation-cache/results-v1.json"),"{broken");
  const report=run(root,["--cache"]);assert.equal(report.cache.reused,0);assert.equal(report.cache.invalidated,true);
  assert.ok(report.cache.warnings.length>0);assert.ok(report.cache.executed>0);
});
test("configuration, negative overrides and SDK execution use the shared cache",{timeout:60000},async t=>{
  const root=fixture(t);
  writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify({cache:true}));
  const cold=run(root),api=await runMutation({node:process.execPath},{binary,cwd:root});
  assert.equal(api.exitCode,0);assert.equal(api.report.cache.reused,cold.summary.killed);
  assert.equal(run(root,["--no-cache"]).cache,undefined);
});
test("reuse works with warmed sessions and time budgets without claiming new trials",{timeout:60000},t=>{
  const root=fixture(t),cold=run(root,["--cache","--reuse-session"]);
  const warm=run(root,["--cache","--reuse-session","--time-budget","0","--fail-on-undetected"]);
  assert.equal(warm.cache.reused,cold.summary.killed);assert.equal(warm.cache.executed,0);
  assert.equal(warm.summary.pending,0);assert.equal(warm.gate.passed,true);
  assert.ok(warm.mutants.every(m=>m.reused));
});
test("cache storage cannot overlap the project root or unrelated existing data",t=>{
  const root=fixture(t);
  const bad=run(root,["--cache","--cache-dir","src"],2);
  assert.equal(bad.testsExecuted,false);assert.equal(bad.diagnostics[0].code,"EC_MUTATION_CONFIG");
  assert.match(bad.diagnostics[0].message,/dedicated/);
  assert.equal(run(root,["--cache","--cache-dir","."],2).testsExecuted,false);
  assert.equal(readFileSync(join(root,"src/value.ts"),"utf8"),"export const add=(a:number,b:number)=>a+b;");
});
for(const kind of ["source","test","dependency","compilation","scope"])test(`reused evidence detects a ${kind} change during final inspection`,{timeout:60000},t=>{
  const root=fixture(t),control=realpathSync(mkdtempSync(join(tmpdir(),"errspec-cache-control-"))),marker=join(control,"enabled");
  t.after(()=>rmSync(control,{recursive:true,force:true}));
  const directory=join(root,"runner"),dist=join(directory,"dist"),vitest=join(repo,"examples/checkout/node_modules/vitest");
  mkdirSync(dist,{recursive:true});
  writeFileSync(join(directory,"package.json"),JSON.stringify({name:"vitest",version:"5.0.5",type:"module",dependencies:{vitest:"5.0.5"}}));
  writeFileSync(join(dist,"index.js"),`export * from ${JSON.stringify(pathToFileURL(join(vitest,"dist/index.js")).href)};`);
  if(kind==="dependency"){
    const dependency=join(root,"node_modules/local-value");mkdirSync(dependency);
    writeFileSync(join(dependency,"package.json"),JSON.stringify({name:"local-value",version:"1.0.0",type:"module"}));
    writeFileSync(join(dependency,"index.js"),'export const unused=1;');
    writeFileSync(join(root,"package.json"),JSON.stringify({type:"module",dependencies:{vitest:"5.0.5","local-value":"1.0.0"}}));
    writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";test("smoke",()=>expect(1).toBe(1));');
  }
  const path=kind==="compilation"?join(control,'define'):join(root,kind==="source"||kind==="scope"?"src/added.ts":kind==="test"?"new.test.ts":"node_modules/local-value/index.js");
  const code=kind==="compilation"?'0':kind==="source"||kind==="scope"?"export const added=true;":kind==="test"?'import {test} from "vitest";test("new",()=>{});':'export const unused=2;';
  writeFileSync(join(dist,"node.js"),`import {existsSync,writeFileSync} from 'node:fs';import {createVitest as actual} from ${JSON.stringify(pathToFileURL(join(vitest,"dist/node.js")).href)};let contexts=0;export async function createVitest(...args){if(++contexts===3&&existsSync(${JSON.stringify(marker)}))writeFileSync(${JSON.stringify(path)},${JSON.stringify(code)});return actual(...args);}`);
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["*.test.ts"]}};');
  if(kind==="compilation"){
    writeFileSync(path,'1');writeFileSync(join(root,'src/value.ts'),'export const add=(value:number)=>value-value;');
    writeFileSync(join(root,'value.test.ts'),`import {test,expect} from 'vitest';import {add} from './src/value';test('cancellation',()=>expect(add(__ERRSPEC_INPUT__)).toBe(0));`);
    writeFileSync(join(root,'vitest.config.mjs'),`import {readFileSync} from 'node:fs';export default {define:{__ERRSPEC_INPUT__:readFileSync(${JSON.stringify(path)},'utf8')},test:{include:['*.test.ts']}};`);
  }
  const args=["--cache","--vitest-package",directory,...(kind==="scope"?["--cache-scope"]:[])];
  const cold=run(root,args);
  if(kind==="dependency")assert.ok(cold.summary.noCoverage>0);else assert.ok(cold.cache.saved>0);
  writeFileSync(marker,"enabled");
  if(kind==="scope")writeFileSync(join(root,"src/value.ts"),"\n"+readFileSync(join(root,"src/value.ts"),"utf8"));
  const report=run(root,args,2);
  assert.equal(report.cache.coverageReused,kind==="scope"?undefined:true,JSON.stringify(report.cache));
  if(kind==="scope")assert.ok(report.cache.scopeReused>0,JSON.stringify(report.cache));
  if(kind==="dependency"){assert.equal(report.cache.reused,0);assert.equal(report.cache.invalidated,true);}
  else assert.ok(report.cache.reused>0,JSON.stringify(report.cache));
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_INPUT");
});
test("Git selection ignores only managed cache artifacts and still validates its source diff",{timeout:60000},t=>{
  const root=fixture(t);
  const git=(...args)=>{const result=spawnSync("git",args,{cwd:root,encoding:"utf8"});assert.equal(result.status,0,result.stderr);};
  writeFileSync(join(root,".gitignore"),"node_modules/\n");
  git("init","--quiet");git("add",".");git("-c","user.name=Fixture","-c","user.email=fixture@example.invalid","-c","commit.gpgSign=false","commit","--quiet","-m","base");
  writeFileSync(join(root,"src/value.ts"),readFileSync(join(root,"src/value.ts"),"utf8")+" // changed\n");
  const cold=run(root,["--cache","--since","HEAD"]),warm=run(root,["--cache","--since","HEAD"]);
  assert.equal(warm.cache.reused,cold.summary.killed);
  assert.deepEqual(warm.selection.changedFiles,["src/value.ts"]);assert.equal(warm.selection.fullRunReason,null);
});
