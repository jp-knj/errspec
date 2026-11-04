import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync,readdirSync,existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync,spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {runMutation} from '../packages/core/index.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=process.env.ERRSPEC_BIN||join(repo,'target/debug/errspec');
function fixture(t,source='export const alpha=true;export const beta=true;export const gamma=true;',body='expect(alpha).toBe(true);expect(beta).toBe(true);expect(gamma).toBe(true);'){
 const root=realpathSync(mkdtempSync(join(tmpdir(),'errspec-parallel-'))),control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-parallel-control-')));
 t.after(()=>{rmSync(root,{recursive:true,force:true});rmSync(control,{recursive:true,force:true});});
 mkdirSync(join(root,'src'));mkdirSync(join(root,'node_modules'));mkdirSync(join(root,'tmp'));
 symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(root,'node_modules/vitest'),'dir');
 writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));writeFileSync(join(root,'src/value.ts'),source);
 writeFileSync(join(root,'value.test.ts'),`import {test,expect,inject} from 'vitest';import * as values from './src/value';import {writeFileSync,readFileSync,readdirSync} from 'node:fs';const control=${JSON.stringify(control)};const {alpha,beta,gamma}=values;test('guards',async()=>{${body}});`);
 writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');return {root,control};
}
function run(root,args=[],expected=0){const result=spawnSync(binary,['mutate','--node',process.execPath,'--format','json',...args],{cwd:root,encoding:'utf8',timeout:90000});assert.ifError(result.error);assert.equal(result.status,expected,result.stderr+result.stdout);return JSON.parse(result.stdout);}
const results=report=>report.mutants.map(m=>[m.key,m.status,m.coveredBy,m.infectedBy,m.attempts.map(a=>[a.status,a.killedBy])]);
test('independent workers execute distinct active mutants at the same time',{timeout:60000},t=>{
 for(const reuse of [false,true]){
  const {root,control}=fixture(t,'export const alpha=true;export const beta=true;',`if(!alpha||!beta){const path=control+'/'+process.pid+'.json';const start=Date.now();writeFileSync(path,JSON.stringify({start}));while(readdirSync(control).length<2&&Date.now()-start<3000)await new Promise(resolve=>setTimeout(resolve,10));const overlap=readdirSync(control).length>=2;writeFileSync(path,JSON.stringify({start,end:Date.now(),overlap}));expect(overlap).toBe(true);}expect(alpha).toBe(true);expect(beta).toBe(true);`);
  const report=run(root,['-j','2',...(reuse?['--reuse-session']:[])]);assert.equal(report.runComplete,true);assert.equal(report.summary.killed,2);assert.deepEqual(report.engine.parallel,{requested:2,workers:2});
  const events=readdirSync(control).map(name=>JSON.parse(readFileSync(join(control,name),'utf8')));assert.equal(events.length,2);assert.ok(events.every(event=>event.overlap),'active mutant executions did not overlap');
 }
});
test('parallel sessions preserve sequential outcomes and isolate module state including equivalent survivors',{timeout:60000},t=>{
 const source='let calls=0;export function sum(a:number,b:number){calls++;return a+b;}export function count(){return calls;}export function zero(n:number){return n+0;}';
 const {root}=fixture(t,source,'expect(values.count()).toBe(0);expect(values.sum(2,3)).toBe(5);expect(values.count()).toBe(1);expect(values.zero(2)).toBe(2);');
 const sequential=run(root,['--reuse-session']),parallel=run(root,['--reuse-session','-j','2']);
 assert.ok(sequential.mutants.some(m=>m.status==='Survived'));assert.deepEqual(results(parallel),results(sequential));assert.equal(parallel.engine.parallel.workers,2);
 const weakSequential=run(root,['--reuse-session','--weak-mutation']),weakParallel=run(root,['--reuse-session','--weak-mutation','-j','2']);assert.deepEqual(results(weakParallel),results(weakSequential));assert.ok(weakParallel.mutants.some(m=>m.reason?.startsWith('not infected')));
});
test('parallel and sequential execution share validated complete inputs and cached probe/result evidence',{timeout:60000},t=>{
 const {root}=fixture(t),inventory=run(root,['--time-budget','0']),key=inventory.mutants[0].key;
 const first=run(root,['--cache','--reuse-session','--only',key]);assert.equal(first.cache.saved,1);
 const filled=run(root,['--cache','--reuse-session','-j','2']);assert.equal(filled.cache.disabledReason,undefined,JSON.stringify(filled.cache));assert.equal(filled.cache.reused,1);assert.equal(filled.cache.executed,2);assert.equal(filled.cache.saved,3);
 const warm=run(root,['--cache','--reuse-session','-j','3','--time-budget','0','--fail-on-undetected']);assert.equal(warm.cache.reused,3);assert.equal(warm.cache.executed,0);assert.equal(warm.cache.coverageReused,true);assert.equal(warm.engine.parallel.workers,0);
 const serial=run(root,['--cache','--reuse-session']);assert.equal(serial.cache.reused,3);assert.equal(serial.engine.parallel,undefined);
});
function heterogeneous(root){writeFileSync(join(root,'vitest.config.mjs'),`const secondary=(process.argv[1].match(/errspec-mutation-/g)||[]).length>1;export default {test:{include:['value.test.ts'],provide:{slot:secondary?'secondary':'primary'}}};`);}
test('different resolved worker inputs disable cold persistence and invalidate reused evidence',{timeout:60000},t=>{
 const {root}=fixture(t);heterogeneous(root);
 const cold=run(root,['--cache','-j','2']);assert.equal(cold.cache.saved,0);assert.match(cold.cache.disabledReason,/different runtime inputs/);assert.equal(cold.summary.killed,3);
 const inventory=run(root,['--time-budget','0']),key=inventory.mutants[0].key;
 run(root,['--cache','--only',key]);
 const reused=run(root,['--cache','-j','2'],2);assert.equal(reused.runComplete,false);assert.equal(reused.summary.score,null);assert.equal(reused.cache.invalidated,true);assert.equal(reused.diagnostics[0].code,'EC_MUTATION_INPUT');
});
test('a worker with different registered tests stops the request without a full-score verdict',{timeout:60000},t=>{
 const {root,control}=fixture(t);heterogeneous(root);const marker=join(control,'running');
 writeFileSync(join(root,'value.test.ts'),`import {test,expect,inject} from 'vitest';import {writeFileSync,existsSync} from 'node:fs';import {alpha,beta,gamma} from './src/value';test('guards',async()=>{if(!alpha||!beta||!gamma){writeFileSync(${JSON.stringify(marker)},'yes');await new Promise(resolve=>setTimeout(resolve,20000));}expect(alpha).toBe(true);expect(beta).toBe(true);expect(gamma).toBe(true)});if(inject('slot')==='secondary')test('extra',async()=>{const start=Date.now();while(!existsSync(${JSON.stringify(marker)})&&Date.now()-start<3000)await new Promise(resolve=>setTimeout(resolve,10));expect(existsSync(${JSON.stringify(marker)})).toBe(true);});`);
 const start=Date.now();
 const report=run(root,['-j','2'],2);assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);assert.match(report.diagnostics[0].message,/Worker probe changed/);
 assert.ok(existsSync(marker));assert.ok(Date.now()-start<8000,'peer failure did not stop an active long-running trial');
});
function start(root,args=[]){
 const child=spawn(binary,['mutate','--node',process.execPath,'--format','json','-j','2',...args],{cwd:root,env:{...process.env,TMPDIR:join(root,'tmp')},stdio:['ignore','pipe','pipe']});
 let stdout='',stderr='';child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);
 const done=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>resolve({code,signal,stdout,stderr}));});return {child,done};
}
async function until(predicate,message){const end=Date.now()+12000;while(!predicate()&&Date.now()<end)await delay(10);assert.ok(predicate(),message);}
function running(pid){try{process.kill(pid,0);return !(process.platform==='linux'&&existsSync(`/proc/${pid}/stat`)&&/\) Z /.test(readFileSync(`/proc/${pid}/stat`,'utf8')));}catch(error){if(error.code==='ESRCH')return false;throw error;}}
for(const signal of ['SIGINT','SIGTERM','SIGKILL'])test(`${signal} terminates every active parallel group and its ordinary descendants`,{timeout:30000},async t=>{
 const {root,control}=fixture(t,'export const alpha=true;export const beta=true;',`if(!alpha||!beta){const {spawn}=await import('node:child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});writeFileSync(control+'/'+process.pid+'.json',JSON.stringify({worker:process.pid,descendant:child.pid}));await new Promise(resolve=>setTimeout(resolve,20000));}expect(alpha).toBe(true);expect(beta).toBe(true);`);
 const unrelated=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'}),execution=start(root);
 t.after(async()=>{unrelated.kill();if(execution.child.exitCode===null&&execution.child.signalCode===null)execution.child.kill('SIGTERM');await execution.done;});
 await until(()=>readdirSync(control).length>=2,'two active parallel trials did not start');
 const groups=readdirSync(control).map(name=>JSON.parse(readFileSync(join(control,name),'utf8')));execution.child.kill(signal);const result=await execution.done;
 if(signal!=='SIGKILL'){assert.equal(result.code,signal==='SIGINT'?130:143,result.stderr+result.stdout);const report=JSON.parse(result.stdout);assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);}
 await until(()=>groups.every(group=>!running(group.worker)&&!running(group.descendant)),'parallel descendants survived');
 assert.equal(readdirSync(join(root,'tmp')).some(name=>name.startsWith('errspec-mutation-')),false);assert.equal(unrelated.exitCode,null);
});
test('timeout confirmation restarts only its worker and leaves another mutant verdict intact',{timeout:60000},t=>{
 const {root}=fixture(t,'export const alpha=true;export const beta=true;','if(!alpha){while(true){}}expect(alpha).toBe(true);expect(beta).toBe(true);');
 const report=run(root,['-j','2','--reuse-session']);assert.equal(report.runComplete,true);assert.equal(report.summary.timeout,1);assert.equal(report.summary.killed,1);
 const timed=report.mutants.find(m=>m.status==='Timeout');assert.deepEqual(timed.attempts.map(a=>a.status),['Timeout','Timeout']);assert.equal(report.mutants.find(m=>m.status==='Killed').attempts.length,1);
});
test('typechecking removes invalid mutants before parallel trials with both supported compiler APIs',{timeout:60000},t=>{
 const {root}=fixture(t,'export const exact:true=true;export const alpha=true;export const beta=true;','expect(alpha).toBe(true);expect(beta).toBe(true);');
 writeFileSync(join(root,'tsconfig.json'),JSON.stringify({compilerOptions:{strict:true,target:'ES2022',module:'ESNext',moduleResolution:'Bundler',skipLibCheck:true},files:['src/value.ts']}));
 for(const major of [6,7]){
  const report=run(root,['-j','2','--typecheck','--typecheck-package',join(repo,`packages/analysis-driver/node_modules/typescript${major}`)]);
  assert.equal(report.runComplete,true);assert.equal(report.summary.compileError,1);assert.equal(report.summary.killed,2);assert.equal(report.engine.parallel.workers,2);
  assert.equal(report.mutants.find(m=>m.status==='CompileError').attempts.length,0);
 }
});
test('parallel budgets leave unscheduled work pending and shard scope remains explicit',{timeout:60000},t=>{
 const {root}=fixture(t),all=run(root,['-j','2','--time-budget','0','--fail-on-undetected'],1);
 assert.equal(all.summary.pending,3);assert.equal(all.summary.score,null);assert.equal(all.engine.parallel.workers,0);assert.equal(all.policy.budget,3);
 const key=all.mutants[0].key,index=Number(BigInt('0x'+key.slice(0,16))%2n)+1;
 const shard=run(root,['-j','2','--shard',index+'/2','--time-budget','0','--fail-on-undetected'],1);
 assert.equal(shard.shard.eligible,3);assert.equal(shard.shard.selected,shard.summary.total);assert.ok(shard.summary.pending>0);assert.equal(shard.summary.score,null);
});
test('CLI, API and configuration share concurrency and invalid counts fail before tests',{timeout:60000},async t=>{
 const {root}=fixture(t);writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({concurrency:2,policy:{timeBudget:0}}));
 const configured=run(root);assert.equal(configured.engine.parallel.requested,2);
 const serial=run(root,['-j','1']);assert.equal(serial.engine.parallel,undefined);
 const api=await runMutation({node:process.execPath,concurrency:3},{binary,cwd:root});assert.equal(api.exitCode,0);assert.equal(api.report.engine.parallel.requested,3);assert.deepEqual(results(api.report),results(configured));
 for(const concurrency of [0,65,1.5]){writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({concurrency}));const bad=run(root,[],2);assert.equal(bad.testsExecuted,false);assert.equal(bad.diagnostics[0].code,'EC_MUTATION_CONFIG');}
});
