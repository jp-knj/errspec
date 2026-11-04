import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';

export async function installedCallgraphSmoke(root,vitestPackage) {
  const require=createRequire(join(root,'package.json'));
  const {runMutation}=await import(pathToFileURL(require.resolve('@errspec/core')).href);
  writeFileSync(join(root,'src/callgraph.ts'),'export function cancel(n:number){return n-n;}\n');
  writeFileSync(join(root,'src/input.ts'),'export function input(){return 0;}\n');
  writeFileSync(join(root,'mutation.test.ts'),'import {test,expect} from "vitest";import {cancel} from "./src/callgraph";import {input} from "./src/input";test("cancels",()=>expect(cancel(input())).toBe(0));');
  const options={node:process.execPath,vitestPackage,cache:true,cacheScope:true,experimentalCallgraph:true};
  const get=async extra=>{
    const result=await runMutation({...options,...extra},{cwd:root});
    assert.equal(result.exitCode,0);assert.equal(result.report.runComplete,true);
    assert.equal(result.report.cache.disabledReason,undefined);
    return result.report;
  };
  const arithmetic=report=>report.mutants.find(m=>m.file==='src/callgraph.ts'&&m.mutator==='ArithmeticOperator');
  assert.equal(arithmetic(await get()).status,'Survived');
  writeFileSync(join(root,'src/input.ts'),'export function input(){return 1;}\n');
  const experimental=await get();
  assert.equal(arithmetic(experimental).status,'Survived');
  assert.equal(arithmetic(experimental).reused,true);
  assert.ok(experimental.cache.callgraphReused>0);
  assert.equal(experimental.cache.experimentalCallgraph,true);
  const strict=await get({experimentalCallgraph:false});
  assert.equal(arithmetic(strict).status,'Killed');
  assert.equal(arithmetic(strict).reused,undefined);
}
