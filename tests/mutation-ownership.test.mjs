import {temporaryProject} from "./support/project.mjs";
import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdirSync,writeFileSync,readFileSync,symlinkSync,readdirSync,existsSync,rmSync,copyFileSync,realpathSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {fileURLToPath} from 'node:url';
import {createConnection} from 'node:net';
import {randomBytes} from 'node:crypto';
import {Worker} from 'node:worker_threads';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=process.env.ERRSPEC_BIN||join(repo,'target/debug/errspec');
const registry=resolve('/tmp',`errspec-workers-v1-${process.getuid()}`);
function fixture(t){
 const root=temporaryProject(t,"errspec-ownership-test-",{canonical:false});
 mkdirSync(join(root,'src'));mkdirSync(join(root,'tmp'));
 writeFileSync(join(root,'package.json'),'{"type":"module"}');
 symlinkSync(join(repo,'examples/checkout/node_modules'),join(root,'node_modules'),'dir');
 writeFileSync(join(root,'src/value.ts'),'export const value=true;');
 writeFileSync(join(root,'value.test.ts'),`import {test,expect} from 'vitest';import {writeFileSync,existsSync} from 'node:fs';import {spawn} from 'node:child_process';import {value} from './src/value';test('active',async()=>{
 if(!existsSync(new URL('./finish',import.meta.url))){
 const descendant=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});
 writeFileSync(new URL('./descendant',import.meta.url),String(descendant.pid));writeFileSync(new URL('./worker',import.meta.url),String(process.pid));
 await new Promise(resolve=>setTimeout(resolve,20000));
 }expect(value).toBe(true);});`);
 return root;
}
function start(root,native=binary,environment={}){
 const child=spawn(native,['mutate','--root',root,'--node',process.execPath,'--format','json'],{env:{...process.env,TMPDIR:join(root,'tmp'),...environment},stdio:['ignore','pipe','pipe']});
 let stdout='',stderr='';child.stdout.on('data',c=>stdout+=c);child.stderr.on('data',c=>stderr+=c);
 const done=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>resolve({code,signal,stdout,stderr}));});
 return {child,done};
}
async function until(predicate,message,timeout=12000){const end=Date.now()+timeout;while(!predicate()&&Date.now()<end)await delay(10);assert.ok(predicate(),message);}
function running(pid,readStat=readFileSync,platform=process.platform){
 try{process.kill(pid,0);if(platform==='linux'&&/\) Z /.test(readStat(`/proc/${pid}/stat`,'utf8')))return false;return true;}catch(error){if(error.code==='ESRCH'||error.code==='ENOENT')return false;throw error;}
}
test('a process disappearing between kill(0) and reading proc is already stopped',()=>{
 assert.equal(running(process.pid,()=>{throw Object.assign(new Error('process exited'),{code:'ENOENT'});},'linux'),false);
 assert.throws(()=>running(process.pid,()=>{throw Object.assign(new Error('permission denied'),{code:'EACCES'});},'linux'),{code:'EACCES'});
});
function record(owner){
 for(const name of readdirSync(registry)){
  if(!/^[a-f0-9]{32}\.json$/.test(name))continue;
  const path=join(registry,name);try{const data=JSON.parse(readFileSync(path,'utf8'));if(data.owner===owner)return {path,data};}catch(error){if(error.code!=='ENOENT')throw error;}
 }
}
async function active(root,t){
 const execution=start(root);t.after(()=>stop(execution));await until(()=>existsSync(join(root,'worker')),'worker did not start');
 const entry=record(execution.child.pid);assert.ok(entry,'supervisor did not register ownership');
 return {...execution,entry,worker:Number(readFileSync(join(root,'worker'),'utf8')),descendant:Number(readFileSync(join(root,'descendant'),'utf8'))};
}
async function stop(execution){if(execution.child.exitCode===null&&execution.child.signalCode===null)execution.child.kill('SIGTERM');await execution.done;}
function sentinel(t){const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});t.after(()=>child.kill());return child;}

test('a supervisor removes its orphaned group and only known temporary files after native SIGKILL',{timeout:30000},async t=>{
 const root=fixture(t),unrelated=sentinel(t),execution=await active(root,t);
 execution.child.kill('SIGKILL');await execution.done;
 await until(()=>!running(execution.entry.data.supervisor)&&!running(execution.worker)&&!running(execution.descendant),'owned descendants survived their native owner');
 assert.equal(unrelated.exitCode,null);assert.equal(existsSync(execution.entry.path),false);assert.equal(existsSync(execution.entry.data.socket),false);
 assert.equal(readdirSync(join(root,'tmp')).some(name=>name.startsWith('errspec-mutation-')),false);
});
test('a subsequent run authenticates and recovers a previous orphan before running new tests',{timeout:30000},async t=>{
 const root=fixture(t),unrelated=sentinel(t),execution=await active(root,t);
 writeFileSync(join(root,'finish'),'yes');
 await new Promise(resolve=>{execution.child.once('exit',resolve);execution.child.kill('SIGKILL');});
 const next=start(root);t.after(()=>stop(next));const result=await next.done;
 assert.equal(result.code,0,result.stderr+result.stdout);const report=JSON.parse(result.stdout);
 assert.equal(report.runComplete,true);assert.ok(report.workers?.recovered>=1,'the previous supervisor was not recovered through its endpoint');
 await until(()=>!running(execution.worker)&&!running(execution.descendant),'recovered workers survived');
 assert.equal(unrelated.exitCode,null);
});
for(const mode of ['active','recycled-pid','wrong-token','corrupt-record'])test(`${mode} records never authorize termination of another live run or unrelated PID`,{timeout:30000},async t=>{
 const root=fixture(t),unrelated=sentinel(t),execution=await active(root,t);
 writeFileSync(join(root,'finish'),'yes');
 if(mode==='recycled-pid')execution.entry.data.supervisor=unrelated.pid;
 if(mode==='wrong-token')execution.entry.data.token=execution.entry.data.token.slice(0,32)+'f'.repeat(32);
 if(mode!=='active')writeFileSync(execution.entry.path,mode==='corrupt-record'?'{bad':JSON.stringify(execution.entry.data));
 const next=start(root);t.after(()=>stop(next));const result=await next.done;
 assert.equal(result.code,0,result.stderr+result.stdout);const report=JSON.parse(result.stdout);
 if(mode==='active')assert.ok(report.workers?.active>=1);else assert.ok(report.workers?.unverified>=1);
 assert.equal(execution.child.exitCode,null);assert.equal(unrelated.exitCode,null);assert.ok(running(execution.worker));
});
test('orphan cleanup preserves unexpected files inside its temporary directory',{timeout:30000},async t=>{
 const root=fixture(t),execution=await active(root,t);
 const name=readdirSync(join(root,'tmp')).find(name=>name.startsWith('errspec-mutation-'));assert.ok(name);
 const directory=join(root,'tmp',name),kept=join(directory,'keep.txt');writeFileSync(kept,'project-created');
 execution.child.kill('SIGKILL');await execution.done;
 await until(()=>!running(execution.worker)&&!running(execution.descendant),'orphan group survived');
 assert.equal(readFileSync(kept,'utf8'),'project-created');assert.deepEqual(readdirSync(directory),['keep.txt']);
});
test('replacing the executable during configuration cannot mix native generation with old ownership evidence',{timeout:30000},async t=>{
 const root=fixture(t);writeFileSync(join(root,'finish'),'yes');mkdirSync(join(root,'engine'));
 const native=join(root,'engine/errspec'),saved=join(root,'engine/original');copyFileSync(binary,native);
 const wrapper=`#!${process.execPath}\nimport {readFileSync} from 'node:fs';import {spawnSync} from 'node:child_process';let input=readFileSync(0);if(process.argv[2]==='__mutation-source'){const data=JSON.parse(input);data.engineFingerprint='0'.repeat(64);input=JSON.stringify(data);}const result=spawnSync(${JSON.stringify(saved)},process.argv.slice(2),{input,encoding:'utf8'});process.stdout.write(result.stdout||'');process.stderr.write(result.stderr||'');process.exitCode=result.status;`;
 writeFileSync(join(root,'vitest.config.mjs'),`import {existsSync,renameSync,writeFileSync} from 'node:fs';if(!existsSync(${JSON.stringify(saved)})){renameSync(${JSON.stringify(native)},${JSON.stringify(saved)});writeFileSync(${JSON.stringify(native)},${JSON.stringify(wrapper)},{mode:0o755});}export default {};`);
 const execution=start(root,native);t.after(()=>stop(execution));const result=await execution.done;
 assert.equal(result.code,2,result.stderr+result.stdout);const report=JSON.parse(result.stdout);
 assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);assert.equal(report.diagnostics[0].code,'EC_MUTATION_INPUT');
 assert.match(report.diagnostics[0].message,/engine changed/);
});
for(const signal of ['SIGINT','SIGKILL'])test(`parent monitoring works without a Unix endpoint after ${signal}`,{timeout:30000},async t=>{
 const root=fixture(t),unrelated=sentinel(t),execution=start(root,binary,{ERRSPEC_WORKER_ENDPOINT:'0'});t.after(()=>stop(execution));
 await until(()=>existsSync(join(root,'worker')),'worker did not start in parent-monitor mode');
 const worker=Number(readFileSync(join(root,'worker'),'utf8')),descendant=Number(readFileSync(join(root,'descendant'),'utf8'));
 assert.equal(record(execution.child.pid),undefined);execution.child.kill(signal);const result=await execution.done;
 if(signal==='SIGINT'){assert.equal(result.code,130,result.stderr+result.stdout);assert.equal(JSON.parse(result.stdout).workers.endpointUnavailable,1);}
 await until(()=>!running(worker)&&!running(descendant),'parent-monitor descendants survived');
 assert.equal(unrelated.exitCode,null);assert.equal(readdirSync(join(root,'tmp')).some(name=>name.startsWith('errspec-mutation-')),false);
});
test('unresponsive endpoints and a full connection backlog cannot stall the next run',{timeout:30000},async t=>{
 const root=fixture(t),unrelated=sentinel(t);writeFileSync(join(root,'finish'),'yes');
 mkdirSync(registry,{recursive:true,mode:0o700});const directory=realpathSync(registry),token=randomBytes(32).toString('hex');
 const socket=join(directory,`${token.slice(0,32)}.sock`),path=join(directory,`${token.slice(0,32)}.json`);
 const listener=new Worker(`const {parentPort,workerData}=require('node:worker_threads');require('node:net').createServer(()=>{}).listen({path:workerData,backlog:1},()=>{parentPort.postMessage('ready');Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,10000);});`,{eval:true,workerData:socket});
 const connections=[];t.after(async()=>{for(const connection of connections)connection.destroy();await listener.terminate();rmSync(path,{force:true});rmSync(socket,{force:true});});
 await new Promise((resolve,reject)=>{listener.once('message',resolve);listener.once('error',reject);});
 for(let i=0;i<16;i++){const connection=createConnection(socket);connection.on('error',()=>{});connections.push(connection);}
 await delay(50);writeFileSync(path,JSON.stringify({version:1,runId:'unresponsiveworker',token,owner:process.pid,supervisor:unrelated.pid,socket}),{mode:0o600});
 const startTime=Date.now(),execution=start(root);t.after(()=>stop(execution));const result=await execution.done;
 assert.equal(result.code,0,result.stderr+result.stdout);assert.ok(JSON.parse(result.stdout).workers?.unverified>=1);
 assert.ok(Date.now()-startTime<8000,'recovery exceeded the connection/response deadline');assert.equal(unrelated.exitCode,null);
});
test('slow ownership requests cannot hold a supervisor after its parent exits',{timeout:30000},async t=>{
 const root=fixture(t),execution=await active(root,t),connection=createConnection(execution.entry.data.socket);
 connection.on('error',()=>{});let timer;
 try{
  await new Promise((resolve,reject)=>{connection.once('connect',resolve);connection.once('error',reject);});
  connection.write('x');timer=setInterval(()=>connection.write('x'),50);await delay(150);
  execution.child.kill('SIGKILL');
  await Promise.race([execution.done,delay(3000).then(()=>{throw Error('slow client blocked orphan termination');})]);
  await until(()=>!running(execution.worker)&&!running(execution.descendant),'slow client kept workers alive');
 }finally{clearInterval(timer);connection.destroy();}
});
test('inherited ignored SIGCHLD cannot bypass owned-child reaping',{timeout:30000},async t=>{
 const root=fixture(t);writeFileSync(join(root,'finish'),'yes');const launcher=join(root,'ignored-child.sh');
 const quote=value=>"'"+value.replaceAll("'","'\\''")+"'";
 writeFileSync(launcher,'#!/bin/sh\ntrap "" CHLD\nexec '+quote(binary)+' "$@"\n',{mode:0o755});
 const execution=start(root,launcher);t.after(()=>stop(execution));const result=await execution.done;
 assert.equal(result.code,0,result.stderr+result.stdout);assert.equal(JSON.parse(result.stdout).runComplete,true);
 assert.equal(record(execution.child.pid),undefined);
});
