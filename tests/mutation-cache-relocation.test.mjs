import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,cpSync,rmSync,symlinkSync,realpathSync,readFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {runMutation} from '../packages/core/index.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=process.env.ERRSPEC_BIN||join(repo,'target/debug/errspec');
function fixture(t,provided=false,localRunner=false){
 const base=realpathSync(mkdtempSync(join(tmpdir(),'errspec-cache-relocation-'))),a=join(base,'a'),b=join(base,'b');t.after(()=>rmSync(base,{recursive:true,force:true}));
 mkdirSync(join(a,'src'),{recursive:true});mkdirSync(join(a,'node_modules/fixturedep'),{recursive:true});
 if(localRunner)cpSync(join(repo,'examples/checkout/node_modules'),join(a,'node_modules'),{recursive:true,verbatimSymlinks:true});
 else symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(a,'node_modules/vitest'),'dir');
 writeFileSync(join(a,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5',fixturedep:'1.0.0'}}));
 writeFileSync(join(a,'node_modules/fixturedep/package.json'),JSON.stringify({name:'fixturedep',version:'1.0.0',type:'module',exports:'./index.js'}));
 writeFileSync(join(a,'node_modules/fixturedep/index.js'),'export const constant=7;');
 writeFileSync(join(a,'src/value.ts'),'export const alpha=true;export const beta=true;');
 writeFileSync(join(a,'setup.mjs'),'globalThis.__fixtureSetup=(globalThis.__fixtureSetup||0)+1;');
 writeFileSync(join(a,'value.test.ts'),`import {test,expect} from 'vitest';import {alpha,beta} from './src/value';import {constant} from 'fixturedep';test('guards',()=>{expect(globalThis.__fixtureSetup).toBe(1);expect(constant).toBe(7);expect(alpha).toBe(true);expect(beta).toBe(true);});`);
 writeFileSync(join(a,'vitest.config.mjs'),`export default {test:{include:['value.test.ts'],setupFiles:['./setup.mjs']${provided?",provide:{location:process.cwd()}":''}}};`);
 return {a,b,copy:()=>cpSync(a,b,{recursive:true,verbatimSymlinks:true})};
}
function run(root,args=[],extraEnv={}){
 const result=spawnSync(binary,['mutate','--node',process.execPath,'--format','json','--cache','--reuse-session',...args],{cwd:root,env:{...process.env,PWD:root,INIT_CWD:root,...extraEnv},encoding:'utf8',timeout:90000});
 assert.ifError(result.error);assert.equal(result.status,0,result.stderr+result.stdout);const report=JSON.parse(result.stdout);assert.equal(report.cache.disabledReason,undefined,JSON.stringify(report.cache));return report;
}
test('opt-in relocation reuses results and probe evidence across checkout paths and rebinds current tests',{timeout:60000},t=>{
 const {a,b,copy}=fixture(t),cold=run(a,['--cache-relocation','-j','2']);copy();
 const warm=run(b,['--cache-relocation','-j','2','--fail-on-undetected']);
 assert.equal(cold.cache.executed,2);assert.equal(warm.cache.relocation,true);assert.equal(warm.cache.reused,2);assert.equal(warm.cache.executed,0);assert.equal(warm.cache.coverageReused,true);assert.equal(warm.cache.fingerprint,cold.cache.fingerprint);
 assert.equal(warm.testsExecuted,true);assert.deepEqual(warm.baseline.map(row=>[row.id,row.status,row.started]),cold.baseline.map(row=>[row.id,row.status,row.started]));assert.equal(warm.engine.parallel.workers,0);assert.equal(warm.summary.killed,2);
 writeFileSync(join(b,'node_modules/fixturedep/index.js'),'// changed dependency contents\nexport const constant=7;');
 const changed=run(b,['--cache-relocation']);assert.equal(changed.cache.reused,0);assert.equal(changed.cache.invalidated,true);assert.equal(changed.cache.executed,2);assert.equal(changed.cache.coverageReused,undefined);
 writeFileSync(join(b,'value.test.ts'),readFileSync(join(b,'value.test.ts'),'utf8').replace("test('guards'","test('renamed'").replace('expect(alpha).toBe(true)','expect(alpha).toBeDefined()').replace('expect(beta).toBe(true)','expect(beta).toBeDefined()'));
 const removed=run(b,['--cache-relocation']);assert.equal(removed.cache.reused,0);assert.equal(removed.cache.invalidated,true);assert.equal(removed.summary.survived,2);
});
test('default cache keeps absolute checkout identity and explicit mode changes cannot reuse old outcomes',{timeout:60000},t=>{
 const {a,b,copy}=fixture(t);const cold=run(a);copy();const moved=run(b);assert.equal(moved.cache.reused,0);assert.notEqual(moved.cache.fingerprint,cold.cache.fingerprint);
 const portable=run(b,['--cache-relocation']);assert.equal(portable.cache.reused,0);assert.equal(portable.cache.invalidated,true);
 const exact=run(b,['--no-cache-relocation']);assert.equal(exact.cache.reused,0);assert.equal(exact.cache.relocation,undefined);
});
test('relocation includes the installed runner dependency tree and Vite filesystem aliases',{timeout:60000},t=>{
 const {a,b,copy}=fixture(t,false,true),cold=run(a,['--cache-relocation']);copy();
 const warm=run(b,['--cache-relocation']);assert.equal(warm.cache.fingerprint,cold.cache.fingerprint);assert.equal(warm.cache.reused,2);assert.equal(warm.cache.coverageReused,true);
});
test('provided absolute values and non-location environment changes remain bound',{timeout:60000},t=>{
 const {a,b,copy}=fixture(t,true);const cold=run(a,['--cache-relocation']);copy();const moved=run(b,['--cache-relocation']);assert.equal(moved.cache.reused,0);assert.notEqual(moved.cache.fingerprint,cold.cache.fingerprint);
 const changed=run(b,['--cache-relocation'],{ERRSPEC_FIXTURE_INPUT:'changed'});assert.equal(changed.cache.reused,0);assert.notEqual(changed.cache.fingerprint,moved.cache.fingerprint);
});
test('configuration and API share relocation and CLI/API false override configured true',{timeout:60000},async t=>{
 const {a}=fixture(t);writeFileSync(join(a,'errspec.mutation.config.json'),JSON.stringify({cache:true,cacheRelocation:true,reuseSession:true}));
 const configured=run(a);assert.equal(configured.cache.relocation,true);
 // API uses the same inherited environment for both calls.
 const first=await runMutation({node:process.execPath},{binary,cwd:a}),second=await runMutation({node:process.execPath},{binary,cwd:a});assert.equal(first.exitCode,0);assert.equal(second.report.cache.relocation,true);assert.equal(second.report.cache.reused,2);
 const exact=await runMutation({node:process.execPath,cacheRelocation:false},{binary,cwd:a});assert.equal(exact.report.cache.relocation,undefined);assert.equal(exact.report.cache.reused,0);
 assert.equal(run(a,['--no-cache-relocation']).cache.relocation,undefined);
});
