import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, symlinkSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { runMutation } from "../packages/core/index.mjs";

const repo=fileURLToPath(new URL("../",import.meta.url)),binary=join(repo,"target/debug/errspec");
const source="export function choose(x:number){\n  const v=x+1;\n  return v>2;\n}\nexport function untouched(x:number){\n  return x-1;\n}\n";
function git(cwd,...args){const r=spawnSync("git",args,{cwd,encoding:"utf8"});assert.equal(r.status,0,r.stderr);return r.stdout.trim();}
function fixture(t,nested=false){
  const directory=realpathSync(mkdtempSync(join(tmpdir(),"errspec-git-selection-")));
  t.after(()=>rmSync(directory,{recursive:true,force:true}));
  const root=nested?join(directory,"project"):directory;
  mkdirSync(join(root,"src"),{recursive:true});mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(directory,".gitignore"),"node_modules/\n");
  writeFileSync(join(root,"src/café space.ts"),source);
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {choose,untouched} from "./src/café space";test("works",()=>{expect(choose(5)).toBe(true);expect(untouched(3)).toBe(2);});');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"]}};');
  writeFileSync(join(root,"README.md"),"original\n");
  git(directory,"init","--quiet");git(directory,"config","user.name","Fixture");git(directory,"config","user.email","fixture@example.invalid");
  git(directory,"add",".");git(directory,"commit","--quiet","-m","baseline");
  return {root,directory};
}
function run(root,args=[],code=0,budget=0){
  const r=spawnSync(binary,["mutate","--node",process.execPath,"--format","json","--time-budget",String(budget),...args],{cwd:root,encoding:"utf8",timeout:30000});
  assert.ifError(r.error);assert.equal(r.status,code,r.stderr+r.stdout);return JSON.parse(r.stdout);
}
test("node and function scope selections use the current AST and retain stable IDs",t=>{
  const {root}=fixture(t),path=join(root,"src/café space.ts"),all=run(root);
  writeFileSync(path,source.replace("v>2","v>=2"));
  const node=run(root,["--since","HEAD"]),scope=run(root,["--since","HEAD","--diff-unit","scope"]);
  assert.ok(node.mutants.length>0);assert.ok(scope.mutants.length>node.mutants.length);
  assert.ok(scope.mutants.every(c=>c.line<=4));
  assert.ok(scope.mutants.some(c=>c.line===2));assert.ok(node.mutants.every(c=>c.line!==2));
  assert.equal(node.selection.unit,"node");assert.equal(scope.selection.unit,"scope");
  assert.equal(node.selection.total,all.mutants.length);
  assert.equal(node.selection.selected+node.selection.omitted,node.selection.total);
  const unchanged=all.mutants.find(c=>c.line===2);
  assert.ok(scope.mutants.some(c=>c.key===unchanged.key));
  assert.equal(readFileSync(path,"utf8"),source.replace("v>2","v>=2"));
});
test("an empty diff selects no mutants, and --only intersects rather than broadens it",t=>{
  const {root}=fixture(t),all=run(root);
  const empty=run(root,["--since","HEAD"]);
  assert.equal(empty.runComplete,true);assert.equal(empty.mutants.length,0);
  assert.equal(empty.selection.omitted,all.mutants.length);
  const only=run(root,["--since","HEAD","--only",all.mutants[0].key]);
  assert.equal(only.selection.total,1);assert.equal(only.mutants.length,0);
});
test("selected mutants execute real trials and omitted functions stay outside the verdict",t=>{
  const {root}=fixture(t),path=join(root,"src/café space.ts");
  writeFileSync(path,source.replace("v>2","v>=2"));
  const report=run(root,["--since","HEAD"],0,30);
  assert.ok(report.summary.killed>0);
  assert.equal(report.summary.pending,0);
  assert.ok(report.mutants.every(c=>c.line<=4&&c.attempts.length>0));
  assert.ok(report.selection.omitted>0);
});
test("untracked sources and new files are selected without matching unrelated sources",t=>{
  const {root}=fixture(t);writeFileSync(join(root,"src/new.ts"),"export const added=true;");
  const report=run(root,["--since","HEAD"]);
  assert.ok(report.mutants.length>0);
  assert.ok(report.mutants.every(c=>c.file==="src/new.ts"));
  assert.equal(report.selection.fullRunReason,null);
});
test("changed and removed tests force rechecking; uncollected changed tests warn",t=>{
  const {root}=fixture(t),all=run(root);
  writeFileSync(join(root,"value.test.ts"),readFileSync(join(root,"value.test.ts"),"utf8")+"\n// changed\n");
  const collected=run(root,["--since","HEAD"]);
  assert.equal(collected.mutants.length,all.mutants.length);assert.equal(collected.selection.warnings.length,0);
  writeFileSync(join(root,"uncollected.test.ts"),"// intentionally excluded\n");
  const excluded=run(root,["--since","HEAD"]);
  assert.equal(excluded.selection.warnings[0].file,"uncollected.test.ts");
  assert.equal(excluded.ok,true,"Warning must not become a quality failure");
  git(root,"add",".");git(root,"commit","--quiet","-m","extra test");
  rmSync(join(root,"uncollected.test.ts"));
  const removed=run(root,["--since","HEAD"]);
  assert.equal(removed.mutants.length,all.mutants.length);
  assert.match(removed.selection.fullRunReason,/Tests changed/);
  assert.equal(removed.selection.warnings[0].file,"uncollected.test.ts");
});
test("dependency/configuration changes select all and unusual Git paths remain exact",t=>{
  const {root}=fixture(t),all=run(root),name="notes with\nnewline.md";
  writeFileSync(join(root,name),"dependency/config changes cannot be ruled out\n");
  git(root,"config","color.ui","always");
  const report=run(root,["--since","HEAD"]);
  assert.equal(report.mutants.length,all.mutants.length);
  assert.deepEqual(report.selection.changedFiles,[name]);
  assert.match(report.selection.fullRunReason,/outside the mutation inventory/);
});
test("monorepo selection is rooted at the package and handles staged changes",t=>{
  const {root,directory}=fixture(t,true);
  writeFileSync(join(directory,"outside.ts"),"export const ignored=true;");
  assert.equal(run(root,["--since","HEAD"]).selection.changedFiles.length,0);
  writeFileSync(join(root,"src/café space.ts"),source.replace("v>2","v>=2"));
  git(directory,"add","project/src/café space.ts");
  const report=run(root,["--since","HEAD"]);
  assert.deepEqual(report.selection.changedFiles,["src/café space.ts"]);
  assert.ok(report.mutants.every(c=>c.file==="src/café space.ts"));
});
test("API and configuration options share the CLI selection, and invalid bases never run tests",async t=>{
  const {root}=fixture(t);
  writeFileSync(join(root,"src/café space.ts"),source.replace("v>2","v>=2"));
  const cli=run(root,["--since","HEAD","--diff-unit","scope"]);
  const api=await runMutation({node:process.execPath,since:"HEAD",diffUnit:"scope",policy:{timeBudget:0}},{binary,cwd:root});
  assert.deepEqual(api.report.selection,cli.selection);assert.deepEqual(api.report.mutants,cli.mutants);
  const bad=run(root,["--since","missing-revision"],2);
  assert.equal(bad.testsExecuted,false);assert.equal(bad.diagnostics[0].code,"EC_MUTATION_CONFIG");
  assert.equal(run(root,["--diff-unit","scope"],2).testsExecuted,false);
  writeFileSync(join(root,"errspec.mutation.config.json"),JSON.stringify({since:"HEAD",diffUnit:"scope",policy:{timeBudget:0}}));
  assert.equal(run(root).selection.unit,"scope");
  assert.equal(run(root,["--diff-unit","node"]).selection.unit,"node");
});
test("changes made during baselines invalidate the diff verdict",t=>{
  const {root}=fixture(t),path=join(root,"value.test.ts");
  writeFileSync(path,readFileSync(path,"utf8")+'\nimport {writeFileSync} from "node:fs";test("writes",()=>writeFileSync(new URL("./README.md",import.meta.url),"changed"));');
  git(root,"add",".");git(root,"commit","--quiet","-m","baseline writes external input");
  const report=run(root,["--since","HEAD"],2);
  assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_INPUT");
});
