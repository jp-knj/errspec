import {temporaryProject,prepareVitestProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from 'node:assert/strict';
import test from 'node:test';
import {writeFileSync,readFileSync} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {runMutation} from '../packages/core/index.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=join(repo,'target/debug/errspec');
const cache=['--cache','--cache-scope'],experiment=[...cache,'--experimental-callgraph'];
function fixture(t,input=0,connected=false){
  const root=temporaryProject(t,"errspec-callgraph-");
  prepareVitestProject(root,repo);

  writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
  writeFileSync(join(root,'src/input.ts'),`export function input(){return ${input};}\n`);
  writeFileSync(join(root,'src/cancel.ts'),connected?'import {input as read} from "./input";export function cancel(){return read()-read();}\n':'export function cancel(n:number){return n-n;}\n');
  writeFileSync(join(root,'value.test.ts'),connected?'import {test,expect} from "vitest";import {cancel} from "./src/cancel";test("cancels",()=>expect(cancel()).toBe(0));':'import {test,expect} from "vitest";import {cancel} from "./src/cancel";import {input} from "./src/input";test("cancels",()=>expect(cancel(input())).toBe(0));');
  writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');return root;
}
function run(root,args=[],code=0){
  return runJsonCommand(binary,['mutate','--node',process.execPath,'--format','json',...args],{cwd:root,encoding:'utf8',timeout:60000},code);
}
const arithmetic=report=>report.mutants.find(m=>m.mutator==='ArithmeticOperator');
function edit(root,input){writeFileSync(join(root,'src/input.ts'),`export function input(){return ${input};}\n`);}
test('experimental calls can prune old survivors and explicitly expose the through-test value-flow limitation',{timeout:60000},t=>{
  const normal=fixture(t),before=run(normal,cache);assert.equal(arithmetic(before).status,'Survived');
  edit(normal,1);const correct=run(normal,cache);assert.equal(arithmetic(correct).status,'Killed');assert.equal(arithmetic(correct).reused,undefined);
  const root=fixture(t),cold=run(root,experiment);assert.equal(arithmetic(cold).status,'Survived');
  edit(root,1);const pruned=run(root,experiment);
  assert.equal(arithmetic(pruned).status,'Survived');assert.equal(arithmetic(pruned).reused,true);
  assert.ok(pruned.cache.callgraphReused>0);assert.equal(pruned.cache.experimentalCallgraph,true);
  assert.ok(pruned.cache.warnings.some(w=>w.startsWith('Experimental static-call')));
  const off=run(root,[...cache,'--no-experimental-callgraph']);
  assert.equal(arithmetic(off).status,'Killed');assert.equal(off.cache.reused,0);assert.equal(off.cache.experimentalCallgraph,undefined);
});
test('old detections still revalidate after disconnected edits even in experimental mode',{timeout:60000},t=>{
  const root=fixture(t,1),cold=run(root,experiment);assert.equal(arithmetic(cold).status,'Killed');
  edit(root,0);const changed=run(root,experiment);
  assert.equal(arithmetic(changed).status,'Survived');assert.equal(arithmetic(changed).reused,undefined);
  assert.equal(changed.cache.callgraphReused,undefined);
});
test('actual Vite import resolution keeps directly connected scopes related',{timeout:60000},t=>{
  const root=fixture(t,0,true),cold=run(root,experiment);assert.equal(arithmetic(cold).status,'Survived');
  const stored=JSON.parse(readFileSync(join(root,'.errspec/mutation-cache/scope-results-v1.json'),'utf8'));
  const owner=stored.context.candidates[arithmetic(cold).key];
  assert.equal(stored.context.staticCalls.nodes[owner].opaque,false,'Known imported alias must be resolved, not merely treated as unknown');
  assert.ok(stored.context.staticCalls.nodes[owner].edges.length>0);
  edit(root,1);const changed=run(root,experiment);
  assert.equal(arithmetic(changed).status,'Killed');assert.equal(arithmetic(changed).reused,undefined);
  assert.equal(changed.cache.callgraphReused,undefined);
});
test('CLI/API/configuration share the opt-in mode and invalid dependencies stop before tests',{timeout:60000},async t=>{
  const root=fixture(t);
  assert.equal(run(root,['--experimental-callgraph'],2).testsExecuted,false);
  assert.equal(run(root,['--cache','--experimental-callgraph'],2).testsExecuted,false);
  writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({cache:true,cacheScope:true,experimentalCallgraph:true}));
  const first=await runMutation({node:process.execPath},{cwd:root,binary});assert.equal(first.report.cache.experimentalCallgraph,true);
  edit(root,1);const next=await runMutation({node:process.execPath},{cwd:root,binary});
  assert.ok(next.report.cache.callgraphReused>0);assert.equal(arithmetic(next.report).status,'Survived');
  const off=await runMutation({node:process.execPath,experimentalCallgraph:false},{cwd:root,binary});
  assert.equal(arithmetic(off.report).status,'Killed');assert.equal(off.report.cache.experimentalCallgraph,undefined);
});
