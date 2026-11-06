import {temporaryProject,prepareVitestProject} from "./support/project.mjs";
import assert from 'node:assert/strict';
import test from 'node:test';
import childProcess, {spawn,spawnSync,execFileSync,execSync} from 'node:child_process';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {observeChildProcesses} from '../packages/mutation-driver/child-process.mjs';
import {runMutation,formatReport} from '../packages/core/index.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=join(repo,'target/debug/errspec');

test('Node diagnostics observe async children, synchronous exports restore exactly, failed launches stay failed',async()=>{
  const original=Object.fromEntries(['spawn','spawnSync','execFileSync','execSync'].map(name=>[name,Object.getOwnPropertyDescriptor(childProcess,name)]));
  const events=[],stop=observeChildProcesses(fact=>events.push(fact));
  assert.equal(childProcess.spawn,original.spawn.value);
  for(const method of ["spawnSync","execFileSync","execSync"]){assert.equal(childProcess[method].name,original[method].value.name);assert.equal(childProcess[method].length,original[method].value.length);}
  try {
    await new Promise((resolve,reject)=>spawn(process.execPath,['-e',''],{env:{...process.env,SECRET_ARGUMENT:'SECRET_VALUE_949'}}).once('close',resolve).once('error',reject));
    assert.equal(spawnSync(process.execPath,['-e','']).status,0);
    assert.equal(execFileSync(process.execPath,['-e','process.stdout.write("yes")'],{encoding:'utf8'}),'yes');
    assert.equal(execSync('printf yes',{encoding:'utf8'}),'yes');
    assert.throws(()=>execFileSync(process.execPath,['-e','process.exit(7)']),error=>error.status===7);
    const failed=spawnSync('/no/errspec/executable',[]);assert.equal(failed.error.code,'ENOENT');
    await new Promise(resolve=>spawn('/no/errspec/executable',[]).once('error',resolve));
    assert.deepEqual(events.map(e=>e.method),['spawn','spawnSync','execFileSync','execSync','execFileSync']);
    assert.ok(events.every(e=>Object.keys(e).sort().join(',')==='argv,cwd,method'));
    assert.doesNotMatch(JSON.stringify(events),/SECRET_ARGUMENT|SECRET_VALUE_949/);
  }finally{stop();stop();}
  for(const name of Object.keys(original))assert.deepEqual(Object.getOwnPropertyDescriptor(childProcess,name),original[name]);
  spawnSync(process.execPath,['-e','']);assert.equal(events.length,5);
});
function project(t) {
  const root=temporaryProject(t,"errspec-child-report-");

  prepareVitestProject(root,repo);
  writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));

  writeFileSync(join(root,'src/leaf.ts'),'export const value = 2 + 3;\n');
  writeFileSync(join(root,'src/effect.ts'),'export const enabled = true;\n');
  writeFileSync(join(root,'src/bridge.ts'),"export {value} from './leaf.ts'; import './effect.ts';\n");
  writeFileSync(join(root,'src/child.ts'),"import {value} from './bridge.ts'; if(value!==5)throw Error('wrong');\n");
  writeFileSync(join(root,'src/inprocess.ts'),'export const enabled = true;\n');
  writeFileSync(join(root,'value.test.ts'),`import {test,expect} from "vitest";
import {spawn,spawnSync,execFileSync,execSync} from "node:child_process";
import {enabled} from "./src/inprocess";
test("sync child",()=>{expect(spawnSync(process.execPath,["src/child.ts","--secret", "PRIVATE_PAYLOAD"],{encoding:"utf8"}).status).toBe(0);});
test("exec file child",()=>{expect(execFileSync(process.execPath,["src/child.ts"],{encoding:"utf8"})).toBe("");});
test("shell child",()=>{expect(execSync(process.execPath+' src/child.ts',{encoding:"utf8"})).toBe("");});
test("async child",async()=>{expect(await new Promise((resolve,reject)=>spawn(process.execPath,["src/child.ts"]).once("close",resolve).once("error",reject))).toBe(0);});
test("in-process",()=>expect(enabled).toBeDefined());
`);
  writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');
  return root;
}
test('actual worker launches produce attributed warnings without raw arguments; reuse preserves observations and gates',async t=>{
  const root=project(t),transport={binary,cwd:root};
  const first=await runMutation({node:process.execPath,cache:true,reuseSession:true,policy:{timeBudget:0} },transport);
  assert.equal(first.exitCode,0);assert.equal(first.report.runComplete,true);
  const facts=first.report.childProcesses;
  assert.equal(facts.length,4);
  assert.deepEqual(new Set(facts.map(f=>f.method)),new Set(['spawnSync','execFileSync','execSync','spawn']));
  assert.ok(facts.every(f=>f.file==='value.test.ts'&&f.entries.join(',')==='src/bridge.ts,src/child.ts,src/effect.ts,src/leaf.ts'&&f.test&&f.executable===(f.method==='execSync'?'sh':'node')),JSON.stringify(facts));
  assert.doesNotMatch(JSON.stringify(first.report),/PRIVATE_PAYLOAD|"argv"|"env"/);
  const agent=JSON.parse(await formatReport(first.report,{reporter:'agent'},transport));
  assert.equal(agent.warnings.filter(w=>w.kind==='childProcess').length,4);
  assert.ok(agent.gaps.filter(g=>g.file==='src/leaf.ts').every(g=>g.childProcess&&g.fix.includes('child process')));
  assert.doesNotMatch(JSON.stringify(agent),/PRIVATE_PAYLOAD/);
  assert.ok(agent.gaps.some(g=>g.file==='src/leaf.ts'&&g.childProcess),'transitive import/re-export target must be attributed');
  const mte=JSON.parse(await formatReport(first.report,{reporter:'json'},transport));
  assert.ok(mte.files['src/child.ts'].mutants.every(m=>m.statusReason.includes('possibly reached')));
  const again=await runMutation({node:process.execPath,cache:true,reuseSession:true,policy:{timeBudget:0}},transport);
  assert.equal(again.report.cache.coverageReused,true,JSON.stringify(again.report.cache));assert.deepEqual(again.report.childProcesses,facts);
  const fail=await runMutation({node:process.execPath,failOnUndetected:true,policy:{timeBudget:0}},transport);
  assert.equal(fail.exitCode,1,'a warning must not silently change the existing gate');
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/inprocess";test("ordinary",()=>expect(enabled).toBeDefined());');
  const ordinary=await runMutation({node:process.execPath,cache:true,policy:{timeBudget:0}},transport);
  assert.equal(ordinary.report.childProcesses,undefined);
  assert.ok(!JSON.parse(await formatReport(ordinary.report,{reporter:'agent'},transport)).warnings.some(w=>w.kind==='childProcess'));
});
test('parallel workers receive the same observation helper before executing their probes',async t=>{
  const root=project(t);
  writeFileSync(join(root,'src/inprocess.ts'),'export function enabled(n:number){return n>0;}');
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/inprocess";test.each([-1,0,1])("in-process %s",n=>expect(enabled(n)).toBe(n>0));');
  const result=await runMutation({node:process.execPath,include:['src/inprocess.ts'],concurrency:2,reuseSession:true},{binary,cwd:root});
  assert.equal(result.exitCode,0,JSON.stringify(result));assert.equal(result.report.runComplete,true);
  assert.equal(result.report.engine.parallel.workers,2);
  assert.ok(result.report.mutants.length>=2);assert.ok(result.report.mutants.every(m=>m.status==='Killed'));
});
