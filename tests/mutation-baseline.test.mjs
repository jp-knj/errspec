import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
const repo=fileURLToPath(new URL("../",import.meta.url));
const binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
test("a later source syntax error preserves earlier acquired candidates in the incomplete report",t=>{
  const root=mkdtempSync(join(tmpdir(),"errspec-batched-source-error-"));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"src/a-valid.ts"),"export const value=true;");
  writeFileSync(join(root,"src/b-invalid.ts"),"export function {");
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {value} from "./src/a-valid";test("value",()=>expect(value).toBe(true));');
  const result=spawnSync(binary,["mutate","--node",process.execPath,"--format","json"],{cwd:root,encoding:"utf8",timeout:30000});
  assert.ifError(result.error);assert.equal(result.status,2,result.stdout+result.stderr);
  const report=JSON.parse(result.stdout);
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_INSTRUMENTATION");
  assert.equal(report.mutants.length,1);assert.equal(report.mutants[0].file,"src/a-valid.ts");
  assert.equal(report.mutants[0].status,"Pending");
});
for(const [name,source,expected] of [
  ["import failure",'import "./missing-dependency";import {test} from "vitest";test("never runs",()=>{});',/missing-dependency/],
  ["failed assertion",'import {test,expect} from "vitest";test("payment boundary",()=>expect("wrong").toBe("expected"));',/payment boundary/],
])test(`baseline diagnostics preserve the cause of ${name}`,t=>{
  const root=mkdtempSync(join(tmpdir(),"errspec-baseline-diagnostic-"));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"src/value.ts"),"export const value=true;");
  writeFileSync(join(root,"value.test.ts"),source);
  const result=spawnSync(binary,["mutate","--node",process.execPath,"--format","json"],{cwd:root,encoding:"utf8",timeout:30000});
  assert.ifError(result.error);assert.equal(result.status,2,result.stdout+result.stderr);
  const report=JSON.parse(result.stdout);
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.match(report.diagnostics[0].message,expected);
  assert.match(report.diagnostics[0].message,/value.test.ts/);
});
test("equivalent test inventories tolerate runner enumeration order changes",t=>{
  const root=mkdtempSync(join(tmpdir(),"errspec-baseline-order-"));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  const vitest=join(repo,"examples/checkout/node_modules/vitest");
  symlinkSync(vitest,join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"src/value.ts"),"export const value=true;");
  for(const name of ["a","b"])writeFileSync(join(root,`${name}.test.ts`),`import {test,expect} from "vitest";import {value} from "./src/value";test(${JSON.stringify(name)},()=>expect(value).toBe(true));`);
  const proxy=join(root,"runner"),dist=join(proxy,"dist");mkdirSync(dist,{recursive:true});
  writeFileSync(join(proxy,"package.json"),JSON.stringify({name:"vitest",version:"5.0.5",type:"module"}));
  writeFileSync(join(dist,"index.js"),`export * from ${JSON.stringify(pathToFileURL(join(vitest,"dist/index.js")).href)};`);
  writeFileSync(join(dist,"node.js"),`
import {createVitest as actual} from ${JSON.stringify(pathToFileURL(join(vitest,"dist/node.js")).href)};
let contexts=0;
export async function createVitest(...args){
 const ctx=await actual(...args),glob=ctx.globTestSpecifications.bind(ctx),reverse=++contexts%2===0;
 ctx.globTestSpecifications=async(...options)=>{
   const specs=(await glob(...options)).sort((a,b)=>a.moduleId.localeCompare(b.moduleId));
   return reverse?specs.reverse():specs;
 };
 return ctx;
}
`);
  const result=spawnSync(binary,["mutate","--node",process.execPath,"--vitest-package",proxy,"--time-budget","0","--format","json"],{cwd:root,encoding:"utf8",timeout:30000});
  assert.ifError(result.error);assert.equal(result.status,0,result.stdout+result.stderr);
  const report=JSON.parse(result.stdout);
  assert.equal(report.runComplete,true);assert.equal(report.baseline.length,2);
  assert.equal(report.summary.pending,1);
});
