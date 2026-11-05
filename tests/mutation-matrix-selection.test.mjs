import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {fileURLToPath} from "node:url";
import {spawnSync} from "node:child_process";
import {runMutation,selectTests} from "../packages/core/index.mjs";
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=process.env.ERRSPEC_BIN||join(repo,'target/debug/errspec');
function git(root,args){const r=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(r.status,0,r.stderr);return r.stdout;}
function fixture(t){
 const root=realpathSync(mkdtempSync(join(tmpdir(),'errspec-matrix-'))),control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-matrix-control-')));t.after(()=>{rmSync(root,{recursive:true,force:true});rmSync(control,{recursive:true,force:true});});
 mkdirSync(join(root,'src'));mkdirSync(join(root,'node_modules'));symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(root,'node_modules/vitest'),'dir');
 writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
 writeFileSync(join(root,'.gitignore'),'node_modules/\n.errspec/\n');
 writeFileSync(join(root,'src/positive.ts'),'export const isPositive=(n:number)=>n>0;');
 writeFileSync(join(root,'src/unrelated.ts'),'export const unrelated=()=>true;');
 writeFileSync(join(root,'other.test.ts'),`import {test,expect} from 'vitest';import {unrelated} from './src/unrelated';test('unrelated',()=>expect(unrelated()).toBe(true));`);
 const tracker=join(control,'events');writeFileSync(tracker,'');
 writeFileSync(join(root,'matrix.test.ts'),`import {test,expect} from 'vitest';import {appendFileSync} from 'node:fs';import {isPositive} from './src/positive';const log=()=>appendFileSync(${JSON.stringify(tracker)},${JSON.stringify('event\n')});
test('early',()=>{log();expect(isPositive(1)).toBe(true)});
test('later',async()=>{log();await new Promise(r=>setTimeout(r,10));expect(isPositive(2)).toBe(true)});
test('zero',()=>{log();expect(isPositive(0)).toBe(false)});
test('wide',()=>{log();expect(isPositive(0)).toBe(false);expect(isPositive(1)).toBe(true);expect(isPositive(-1)).toBe(false)});
test('observes without assertion',()=>{log();isPositive(1)});`);
 writeFileSync(join(root,'vitest.config.mjs'),`export default {test:{include:['*.test.ts'],bail:1,retry:0}};`);
 git(root,['init','-q','-b','main']);git(root,['add','.']);git(root,['-c','commit.gpgSign=false','-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','base']);
 return {root,tracker};
}
function cli(root,command,args=[],code=0){const r=spawnSync(binary,[command,'--node',process.execPath,'--reuse-session','--format','json',...args],{cwd:root,encoding:'utf8',timeout:60000,killSignal:'SIGKILL'});assert.ifError(r.error);assert.equal(r.status,code,r.stdout+r.stderr);return JSON.parse(r.stdout);}
const identities=report=>report.mutants.map(m=>({key:m.key,status:m.status,coveredBy:m.coveredBy,killedBy:m.killedBy,matrix:m.matrix}));
test('complete matrix rejects experimental survivor pruning before any tests start',t=>{
 const {root,tracker}=fixture(t);
 const result=cli(root,'mutate',['--cache','--cache-scope','--experimental-callgraph','--kill-matrix'],2);
 assert.equal(result.testsExecuted,false);assert.equal(result.runComplete,false);
 assert.match(result.diagnostics[0].message,/killMatrix cannot use experimentalCallgraph/);
 assert.equal(readFileSync(tracker,'utf8'),'');
});
test('complete matrix runs late killers despite configured bail/retries, analyzes duplicate/subsumed/no-kill tests, and never reuses first detection',async t=>{
 const {root,tracker}=fixture(t);
 const first=cli(root,'mutate',['--cache']);assert.ok(first.mutants.some(m=>m.killedBy.length===1));assert.equal(first.killMatrix,undefined);
 const resultCache=join(root,'.errspec/mutation-cache/results-v1.json'),firstDetection=JSON.parse(readFileSync(resultCache,'utf8'));
 writeFileSync(join(root,'vitest.config.mjs'),`export default {test:{include:['*.test.ts'],bail:1,retry:2}};`);
 const complete=cli(root,'mutate',['--cache','--kill-matrix','--weak-mutation','-j','2']);
 assert.equal(complete.runComplete,true);assert.equal(complete.killMatrix.acquisition,'all_covering_tests');assert.equal(complete.killMatrix.mutants.full,complete.mutants.length);assert.equal(complete.cache.reused,0);
 const arrow=complete.mutants.find(m=>m.file==='src/positive.ts'&&m.mutator==='ArrowFunction');
 assert.equal(arrow.killedBy.length,4);assert.ok(arrow.killedBy.some(id=>id.includes('later')));assert.equal(arrow.matrix.executed.length,5);assert.equal(arrow.matrix.complete,true);
 assert.equal(complete.killMatrix.noKills.length,1);assert.ok(complete.killMatrix.noKills[0].includes('observes without assertion'));
 assert.ok(complete.killMatrix.duplicates.some(group=>group.tests.some(id=>id.includes('early'))&&group.tests.some(id=>id.includes('later'))));
 assert.ok(complete.killMatrix.subsumed.some(row=>row.test.includes('early')&&row.by.some(id=>id.includes('wide'))));
 // Even a compatible input stamp cannot turn first-detection acquisition into a full matrix.
 firstDetection.fingerprint=complete.cache.fingerprint;writeFileSync(resultCache,JSON.stringify(firstDetection));
 const before=readFileSync(tracker,'utf8').split('\n').length;
 const warm=cli(root,'mutate',['--cache','--kill-matrix','--weak-mutation']);assert.equal(warm.cache.reused,0);assert.ok(warm.cache.executed>0);assert.equal(warm.cache.coverageReused,true);assert.ok(readFileSync(tracker,'utf8').split('\n').length-before>5);
 assert.equal(warm.mutants.find(m=>m.key===arrow.key).killedBy.length,4);assert.equal(warm.cache.saved,0);
 const api=await runMutation({node:process.execPath,reuseSession:true,killMatrix:true,weakMutation:true},{binary,cwd:root});assert.equal(api.exitCode,0);assert.deepEqual(identities(api.report),identities(complete));
 const partial=cli(root,'mutate',['--kill-matrix','--time-budget','0']);assert.equal(partial.killMatrix.mutants.full,0);assert.ok(partial.killMatrix.mutants.incomplete>0);assert.deepEqual(partial.killMatrix.noKills,[]);assert.ok(partial.killMatrix.rows.every(row=>!row.complete&&row.executed.length===0));
 writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({killMatrix:true}));assert.equal(cli(root,'mutate',['--no-kill-matrix']).killMatrix,undefined);
});
test('selectTests acquires current coverage, ranks historical hints, matches files/JSON/API, and drops deleted killers',async t=>{
 const {root,tracker}=fixture(t);cli(root,'mutate',['--cache']);
 writeFileSync(join(root,'src/positive.ts'),'export const isPositive=(n:number)=>/* changed scope */n>0;');
 const events=readFileSync(tracker,'utf8').split('\n').length;
 const command=cli(root,'select-tests',['--since','HEAD','--cache']);assert.equal(command.kind,'errspec/test-selection');assert.equal(command.runComplete,true);assert.deepEqual(command.files,['matrix.test.ts']);assert.ok(command.killers.some(pick=>pick.test.includes('early')&&pick.evidence==='historical_kill_hint'));assert.ok(command.covering.some(pick=>pick.test.includes('later')));
 assert.ok(readFileSync(tracker,'utf8').split('\n').length-events<=10,'selection executes plain/probe only, without mutation trials');
 const api=await selectTests({node:process.execPath,since:'HEAD',cache:true,reuseSession:true},{binary,cwd:root});assert.equal(api.exitCode,0);assert.deepEqual({...api.report,coverage:null},{...command,coverage:null});
 const files=spawnSync(binary,['select-tests','--node',process.execPath,'--since','HEAD','--format','files'],{cwd:root,encoding:'utf8',timeout:30000});assert.equal(files.status,0,files.stderr);assert.equal(files.stdout,'matrix.test.ts\n');
 const testPath=join(root,'matrix.test.ts');writeFileSync(testPath,readFileSync(testPath,'utf8').replace("test('early',()=>{log();expect(isPositive(1)).toBe(true)});",''));
 const deleted=cli(root,'select-tests',['--since','HEAD','--cache']);assert.ok(![...deleted.killers,...deleted.covering].some(pick=>pick.test?.includes('early')));assert.ok(deleted.files.includes('other.test.ts'));assert.ok(deleted.changedTests.some(pick=>pick.file==='matrix.test.ts'));
 rmSync(join(root,'other.test.ts'));const removed=cli(root,'select-tests',['--since','HEAD']);assert.ok(!removed.files.includes('other.test.ts'));assert.ok(removed.warnings.some(w=>w.file==='other.test.ts'&&w.code==='EC_MUTATION_CHANGED_TEST_UNCOLLECTED'));
});
test('selection cannot return a false empty set for unobserved/deleted source or helper edits and requires a valid Git base',async t=>{
 const {root}=fixture(t);
 const missing=await selectTests({node:process.execPath},{binary,cwd:root});assert.equal(missing.exitCode,2);assert.match(missing.report.diagnostics[0].message,/requires since/);
 const invalid=cli(root,'select-tests',['--since','not-a-ref'],2);assert.equal(invalid.runComplete,false);
 writeFileSync(join(root,'src/unobserved.ts'),'export function unobserved(){return 7;}');
 const uncovered=cli(root,'select-tests',['--since','HEAD']);assert.ok(uncovered.files.includes('matrix.test.ts')&&uncovered.files.includes('other.test.ts'));assert.match(uncovered.fallbackReason,/no attributable|outside/);
 writeFileSync(join(root,'helper.txt'),'changed external helper');const helper=cli(root,'select-tests',['--since','HEAD']);assert.equal(helper.files.length,2);assert.ok(helper.fallbackReason);
});
test('removed functions and changed functions with no eligible candidates select all current files',t=>{
 const {root}=fixture(t),path=join(root,'src/positive.ts');
 writeFileSync(path,'export const isPositive=(n:number)=>n>0;\nexport const unobserved=()=>7;\n');
 git(root,['add','src/positive.ts']);git(root,['-c','commit.gpgSign=false','-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','unused function']);
 writeFileSync(path,'export const isPositive=(n:number)=>n>0;\n');
 const removed=cli(root,'select-tests',['--since','HEAD']);assert.equal(removed.files.length,2);assert.match(removed.fallbackReason,/Removed source/);
 writeFileSync(path,'export const isPositive=(n:number)=>/* modified */n>0;\nexport const unobserved=()=>8;\n');
 const noCandidates=cli(root,'select-tests',['--since','HEAD','--exclude-mutator','ArrowFunction']);assert.equal(noCandidates.files.length,2);assert.match(noCandidates.fallbackReason,/no attributable/);
});
test('per-test retries and timeout outcomes remain incomplete matrix rows',t=>{
 const {root}=fixture(t);
 writeFileSync(join(root,'src/positive.ts'),'export const isPositive=()=>true;');
 writeFileSync(join(root,'matrix.test.ts'),`import {test,expect} from 'vitest';import {isPositive} from './src/positive';let attempts=0;test('retried',{retry:1},()=>{if(attempts++===0)throw Error('first try');expect(isPositive()).toBe(true)});`);
 const retried=cli(root,'mutate',['--kill-matrix']);const rows=retried.killMatrix.rows.filter(row=>retried.mutants.find(m=>m.key===row.key).file==='src/positive.ts');
 assert.ok(rows.length>0);assert.ok(rows.every(row=>!row.complete&&/retried|repeated/.test(row.incompleteReason)));
 writeFileSync(join(root,'src/positive.ts'),'export const isPositive=(n:number)=>n>0;');
 writeFileSync(join(root,'matrix.test.ts'),`import {test,expect} from 'vitest';import {isPositive} from './src/positive';test('deadline',async()=>{const value=isPositive(1);if(value!==true)await new Promise(()=>{});expect(value).toBe(true)},20);`);
 const timed=cli(root,'mutate',['--kill-matrix']);assert.ok(timed.killMatrix.mutants.timeout>0);assert.ok(timed.killMatrix.rows.filter(row=>row.status==='Timeout').every(row=>!row.complete));
});
test('selection rejects new collected tests introduced during the final probe even when Git ignores them',t=>{
 const {root}=fixture(t);
 writeFileSync(join(root,'.gitignore'),readFileSync(join(root,'.gitignore'),'utf8')+'new.test.ts\n');
 writeFileSync(join(root,'matrix.test.ts'),`import {test,expect,TestRunner} from 'vitest';import {writeFileSync} from 'node:fs';import {isPositive} from './src/positive';test('adds a test during probe',()=>{expect(isPositive(1)).toBe(true);if(Object.keys(TestRunner.getCurrentTest().meta.__errspecMutation?.hits||{}).length)writeFileSync(${JSON.stringify(join(root,'new.test.ts'))},"import {test} from 'vitest';test('new',()=>{});")});`);
 const changed=cli(root,'select-tests',['--since','HEAD'],2);assert.equal(changed.runComplete,false);assert.deepEqual(changed.files,[]);assert.equal(changed.diagnostics[0].code,'EC_MUTATION_INPUT');assert.match(changed.diagnostics[0].message,/inventory changed/);
});
