import assert from "node:assert/strict";
import test from "node:test";
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {runMutation} from "../packages/core/index.mjs";
const repo=fileURLToPath(new URL("../",import.meta.url)),binary=join(repo,"target/debug/errspec");
const source="export function add(a:number,b:number){return a+b;}\nexport function subtract(a:number,b:number){return a-b;}\n";
function fixture(t){
  const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-scope-cache-")));t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"module",dependencies:{vitest:"5.0.5"}}));
  writeFileSync(join(root,"src/value.ts"),source);
  writeFileSync(join(root,"a.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test("adds",()=>expect(add(2,3)).toBe(5));');
  writeFileSync(join(root,"b.test.ts"),'import {test,expect} from "vitest";import {subtract} from "./src/value";test("subtracts",()=>expect(subtract(5,3)).toBe(2));');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["*.test.ts"]}};');
  return root;
}
function run(root,args=[],code=0,environment={}){
  const defaults=[...(args.includes("--no-cache")?[]:["--cache"]),...(args.includes("--no-cache-scope")||args.includes("--no-cache")?[]:["--cache-scope"])];
  const r=spawnSync(binary,["mutate","--node",process.execPath,"--format","json",...defaults,...args],{cwd:root,encoding:"utf8",timeout:60000,env:{...process.env,...environment}});
  assert.ifError(r.error);assert.equal(r.status,code,r.stdout+r.stderr);return JSON.parse(r.stdout);
}
const verdicts=report=>report.mutants.map(m=>[m.key,m.status,m.coveredBy,m.killedBy]);
test("position-only edits reuse assertion results but refresh all current probe evidence",{timeout:60000},t=>{
  const root=fixture(t),cold=run(root);
  assert.equal(cold.cache.disabledReason,undefined,JSON.stringify(cold.cache));
  assert.equal(cold.cache.scope,true);assert.ok(cold.cache.executed>0);
  writeFileSync(join(root,"src/value.ts"),"\n\n"+source);
  const moved=run(root);
  assert.equal(moved.cache.coverageReused,undefined);assert.equal(moved.cache.executed,0);
  assert.equal(moved.cache.scopeReused,cold.cache.executed);assert.equal(moved.cache.reused,cold.cache.executed);
  assert.deepEqual(verdicts(moved),verdicts(cold));assert.equal(moved.mutants[0].line,cold.mutants[0].line+2);
  const warm=run(root);assert.equal(warm.cache.coverageReused,true);assert.equal(warm.cache.executed,0);
  assert.equal(warm.cache.scopeReused,undefined,"exact matches use ordinary evidence first");
});
test("editing one function reuses only unchanged functions with disjoint test-file cohorts",{timeout:60000},t=>{
  const root=fixture(t),cold=run(root);
  const add=cold.mutants.filter(m=>m.line===1&&m.status==="Killed");assert.ok(add.length>0);
  writeFileSync(join(root,"src/value.ts"),source.replace("return a-b;","return a-b+0;"));
  const partial=run(root,["--concurrency","2"]);
  assert.equal(partial.cache.disabledReason,undefined,JSON.stringify(partial.cache));
  assert.equal(partial.cache.scopeReused,add.length);
  assert.ok(partial.cache.executed>0);assert.ok(partial.mutants.filter(m=>m.reused).every(m=>m.line===1));
  const fresh=run(root,["--no-cache"]);assert.deepEqual(verdicts(partial),verdicts(fresh));
  assert.equal(run(root).cache.executed,0);
});
test("expression-arrow entry guards retain static initialization cohorts",{timeout:60000},t=>{
  const root=fixture(t),text="export const add=(a:number,b:number)=>a+b;\nexport const subtract=(a:number,b:number)=>a-b;\n";
  writeFileSync(join(root,"src/value.ts"),text);
  writeFileSync(join(root,"a.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";const result=add(2,3);test("adds statically",()=>expect(result).toBe(5));');
  const cold=run(root),original=cold.mutants.filter(m=>m.line===1);
  assert.ok(original.every(m=>m.status==="Killed"));
  writeFileSync(join(root,"src/value.ts"),text.replace("=>a-b;","=>a-b+0;"));
  const changed=run(root);assert.equal(changed.cache.scopeReused,original.length);
  assert.deepEqual(verdicts(changed),verdicts(run(root,["--no-cache"])));
});
test("a changed function reached through test code revalidates old detections and can reveal survivors",{timeout:60000},t=>{
  const root=fixture(t),text="export function cancel(n:number){return n-n;}\nexport function input(){return 1;}\n";
  writeFileSync(join(root,"src/value.ts"),text);rmSync(join(root,"b.test.ts"));
  writeFileSync(join(root,"a.test.ts"),'import {test,expect} from "vitest";import {cancel,input} from "./src/value";test("cancels",()=>expect(cancel(input())).toBe(0));');
  const cold=run(root),before=cold.mutants.find(m=>m.mutator==="ArithmeticOperator"&&m.original==="n-n");
  assert.equal(before.status,"Killed");
  writeFileSync(join(root,"src/value.ts"),text.replace("return 1;","return 0;"));
  const changed=run(root),after=changed.mutants.find(m=>m.key===before.key);
  assert.equal(after.status,"Survived");assert.equal(after.reused,undefined);assert.equal(changed.cache.reused,0);
  assert.deepEqual(verdicts(changed),verdicts(run(root,["--no-cache"])));
});
test("different tests sharing a file and unavailable entry guards do not prove independence",{timeout:60000},t=>{
  for(const missingEntry of [false,true]){
    const root=fixture(t),args=missingEntry?["--exclude-mutator","BlockStatement"]:[];
    if(!missingEntry){
      writeFileSync(join(root,"a.test.ts"),readFileSync(join(root,"a.test.ts"),"utf8")+readFileSync(join(root,"b.test.ts"),"utf8").replace('import {test,expect} from "vitest";',''));
      rmSync(join(root,"b.test.ts"));
    }
    run(root,args);writeFileSync(join(root,"src/value.ts"),source.replace("return a-b;","return a-b+0;"));
    const changed=run(root,args);assert.equal(changed.cache.reused,0);assert.ok(changed.cache.executed>0);
  }
});
test("compiler comments, module initialization, helpers and test deletion invalidate scope evidence",{timeout:90000},t=>{
  for(const mode of ["comment","module","helper","removed-test"]){
    const root=fixture(t);
    if(mode==="comment")writeFileSync(join(root,"src/value.ts"),"// compiler-sensitive trivia\n"+source);
    if(mode==="helper")writeFileSync(join(root,"helper.ts"),"export const value=1;");
    run(root);
    if(mode==="comment")writeFileSync(join(root,"src/value.ts"),"\n// compiler-sensitive trivia\n"+source);
    if(mode==="module")writeFileSync(join(root,"src/value.ts"),"export const initialized=Date.now();\n"+source);
    if(mode==="helper")writeFileSync(join(root,"helper.ts"),"export const value=2;");
    if(mode==="removed-test")rmSync(join(root,"b.test.ts"));
    const changed=run(root);assert.equal(changed.cache.reused,0,mode);assert.ok(changed.cache.executed>0,mode);
  }
});
test("corrupt scoped records cannot become detections and scope configuration has explicit negative overrides",{timeout:60000},async t=>{
  const root=fixture(t);run(root);
  writeFileSync(join(root,".errspec/mutation-cache/scope-results-v1.json"),"{bad");
  writeFileSync(join(root,"src/value.ts"),"\n"+source);
  const corrupt=run(root);assert.equal(corrupt.cache.reused,0);assert.ok(corrupt.cache.warnings.some(w=>w.startsWith("Scope cache")));
  writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify({cache:true,cacheScope:true}));
  run(root);writeFileSync(join(root,"src/value.ts"),"\n\n"+source);
  const api=await runMutation({node:process.execPath},{cwd:root,binary});assert.equal(api.report.cache.scope,true);assert.ok(api.report.cache.scopeReused>0);
  writeFileSync(join(root,"src/value.ts"),"\n\n\n"+source);
  const off=run(root,["--no-cache-scope"]);assert.equal(off.cache.scope,undefined);assert.equal(off.cache.reused,0);
  assert.equal(run(root,["--no-cache"]).cache,undefined);
});
test("scope hits preserve partial-budget uncertainty and carry unselected records only under unchanged contexts",{timeout:60000},t=>{
  const root=fixture(t),cold=run(root);
  writeFileSync(join(root,"src/value.ts"),source.replace("return a-b;","return a-b+0;"));
  const budget=run(root,["--time-budget","0"]);
  assert.ok(budget.cache.reused>0);assert.equal(budget.cache.executed,0);
  assert.ok(budget.summary.pending>0);assert.equal(budget.summary.score,null);
  assert.equal(budget.cache.saved,budget.cache.reused,"changed contexts discard unverified old results");
  const filled=run(root);assert.ok(filled.cache.executed>0);assert.ok(filled.cache.reused>0);
  assert.equal(filled.summary.pending,0);
  writeFileSync(join(root,"src/value.ts"),"\n"+source.replace("return a-b;","return a-b+0;"));
  const shard=run(root,["--shard","1/2"]);assert.equal(shard.cache.executed,0);
  const warm=run(root);assert.equal(warm.cache.executed,0);assert.deepEqual(verdicts(warm),verdicts(filled));
  assert.ok(cold.cache.saved>0);
});
for(const mode of ["weak","typecheck"])test(`position reuse keeps current ${mode} judgments separate from old assertion outcomes`,{timeout:60000},t=>{
  const root=fixture(t),args=mode==="weak"?["--weak-mutation"]:["--typecheck","--typecheck-package",join(repo,"packages/analysis-driver/node_modules/typescript6")];
  if(mode==="weak")writeFileSync(join(root,"a.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/value";test("adds zero",()=>expect(add(2,0)).toBe(2));');
  else{
    writeFileSync(join(root,"src/value.ts"),source.replaceAll("b:number){","b:number):number{"));
    writeFileSync(join(root,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,target:"ES2022",module:"ESNext",moduleResolution:"Bundler",skipLibCheck:true},files:["src/value.ts"]}));
  }
  const cold=run(root,args);
  writeFileSync(join(root,"src/value.ts"),"\n"+readFileSync(join(root,"src/value.ts"),"utf8"));
  const moved=run(root,args);assert.equal(moved.cache.executed,0);assert.equal(moved.cache.scopeReused,cold.cache.executed);
  assert.deepEqual(verdicts(moved),verdicts(cold));
  if(mode==="weak")assert.ok(moved.mutants.some(m=>m.reason?.startsWith("not infected")&&!m.reused&&!m.attempts.length));
  else{assert.ok(moved.typecheck.checked>0);assert.ok(moved.mutants.some(m=>m.status==="CompileError"&&!m.reused&&!m.attempts.length));}
});
