import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import assert from 'node:assert/strict';

/** Offline installed-core acceptance shared by the three Distribution platforms. */
export function installedMatrixSmoke(directory,run,runnerPackage) {
  const source='export const isPositive=(n:number)=>n>0;\n';
  writeFileSync(join(directory,'src/mutation.ts'),source);
  writeFileSync(join(directory,'mutation.test.ts'),`import {test,expect} from 'vitest';import {isPositive} from './src/mutation';test('early',()=>expect(isPositive(1)).toBe(true));test('late',()=>expect(isPositive(2)).toBe(true));test('observe',()=>{isPositive(1)});`);
  writeFileSync(join(directory,'vitest.config.mjs'),`export default {test:{include:['mutation.test.ts'],bail:1,retry:0}};`);
  run('git',['init','-q']);
  run('git',['add','src/mutation.ts','mutation.test.ts','vitest.config.mjs']);
  run('git',['-c','commit.gpgSign=false','-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','installed matrix base']);
  writeFileSync(join(directory,'matrix-api.mjs'),`
import assert from 'node:assert/strict';
import {runMutation} from '@errspec/core';
const result=await runMutation({node:process.execPath,vitestPackage:${JSON.stringify(runnerPackage)},killMatrix:true,weakMutation:true,reuseSession:true,cache:true,cacheDir:'matrix-cache'});
assert.equal(result.exitCode,0,JSON.stringify(result));
assert.equal(result.report.killMatrix.mutants.full,result.report.mutants.length);
const arrow=result.report.mutants.find(m=>m.mutator==='ArrowFunction');
assert.equal(arrow.killedBy.length,2);assert.ok(arrow.killedBy.some(id=>id.includes('late')));
assert.equal(arrow.matrix.complete,true);assert.equal(arrow.matrix.executed.length,3);
const survivor=result.report.mutants.find(m=>m.status==='Survived');assert.equal(survivor.matrix.executed.length,3);assert.equal(survivor.attempts.length,1);
assert.equal(result.report.cache.reused,0);
if(process.argv.includes('--warm'))assert.equal(result.report.cache.coverageReused,true);
`);
  run(process.execPath,['matrix-api.mjs']);run(process.execPath,['matrix-api.mjs','--warm']);
  const text=run(process.execPath,[join(directory,'node_modules/@errspec/cli/bin/errspec.mjs'),'mutate','--kill-matrix','--node',process.execPath,'--vitest-package',runnerPackage,'--reuse-session']);
  assert.match(text,/kill matrix: [1-9][0-9]* complete, 0 timeout, 0 incomplete/);assert.match(text,/no kills:.*observe/);assert.match(text,/duplicate kill sets/);
  writeFileSync(join(directory,'src/mutation.ts'),source.replace('=>n','=>/* changed */n'));
  writeFileSync(join(directory,'selection-api.mjs'),`
import assert from 'node:assert/strict';import {selectTests} from '@errspec/core';
const result=await selectTests({node:process.execPath,vitestPackage:${JSON.stringify(runnerPackage)},since:'HEAD',reuseSession:true});
assert.equal(result.exitCode,0,JSON.stringify(result));assert.equal(result.report.kind,'errspec/test-selection');
assert.equal(result.report.currentTests,3);assert.deepEqual(result.report.files,['mutation.test.ts']);
`);
  run(process.execPath,['selection-api.mjs']);
}
