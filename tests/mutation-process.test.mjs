import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, symlinkSync, readdirSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
const root=new URL("../",import.meta.url).pathname;
const binary=process.env.ERRSPEC_BIN||resolve(root,"target/debug/errspec");
const families=["ArithmeticOperator","ArrayDeclaration","ArrowFunction","AssignmentOperator","BlockStatement","CallExpression","ConditionalExpression","EqualityOperator","FnValue","LogicalOperator","MethodExpression","ObjectLiteral","OptionalChaining","Regex","StringLiteral","UnaryOperator","UpdateOperator"];
function fixture(body) {
  const directory=mkdtempSync(join(tmpdir(),"errspec-mutation-process-"));
  mkdirSync(join(directory,"src"));mkdirSync(join(directory,"tmp"));
  writeFileSync(join(directory,"package.json"),'{"type":"module"}');
  writeFileSync(join(directory,"src/value.ts"),'export const value=true;');
  writeFileSync(join(directory,"x.test.ts"),'import {test,expect} from "vitest";import {writeFileSync,existsSync} from "node:fs";import {value} from "./src/value";test("active",async()=>{writeFileSync(new URL("./started",import.meta.url),"ready");'+body+'});');
  symlinkSync(resolve(root,"examples/checkout/node_modules"),join(directory,"node_modules"),"dir");
  return directory;
}
function start(directory, traceGenerator=false, args=[]) {
  const env={...process.env,TMPDIR:join(directory,"tmp")};
  if(traceGenerator){
    const hook=join(directory,"trace-generator.mjs");
    writeFileSync(hook,`import cp from 'node:child_process';import {appendFileSync} from 'node:fs';import {syncBuiltinESMExports} from 'node:module';
const original=cp.spawn;cp.spawn=function(binary,args,options){const generator=args?.[0]==='__mutation-source-lines';if(generator&&${traceGenerator==="corrupt"}){binary=process.execPath;args=['-e',"process.stdin.once('data',()=>console.log('null'));"];}const child=original.call(this,binary,args,options);if(generator)appendFileSync(${JSON.stringify(join(directory,"generator.pid"))},String(child.pid)+"\\n");return child;};syncBuiltinESMExports();`);
    env.NODE_OPTIONS=`--import=${JSON.stringify(hook)}`;
  }
  const child=spawn(binary,["mutate","--root",directory,"--node",process.execPath,"--format","json",...args,...(process.env.ERRSPEC_REUSE_SESSION?["--reuse-session"]:[]),...families.flatMap(f=>["--exclude-mutator",f])],
    {env,stdio:["ignore","pipe","pipe"]});
  let stdout="",stderr="";child.stdout.on("data",c=>stdout+=c);child.stderr.on("data",c=>stderr+=c);
  const done=new Promise((resolve,reject)=>{child.on("error",reject);child.on("close",(code,signal)=>resolve({code,signal,stdout,stderr}));});
  return {child,done};
}
function assertClean(directory){
  const temporary=join(directory,"tmp"),registry=`errspec-mutation-owners-${process.getuid()}`;
  assert.deepEqual(readdirSync(temporary).filter(name=>name.startsWith("errspec-mutation-")&&name!==registry),[]);
  if(existsSync(join(temporary,registry)))assert.deepEqual(readdirSync(join(temporary,registry)),[]);
  if(existsSync(join(directory,"generator.pid"))){
    for(const pid of readFileSync(join(directory,"generator.pid"),"utf8").trim().split("\n").map(Number))
      assert.throws(()=>process.kill(pid,0),error=>error.code==="ESRCH","Generator is still alive");
  }
}
for(const [signal,exit] of [["SIGINT",130],["SIGTERM",143]])test(`${signal} terminates owned workers and removes temporary driver state`,{timeout:30000},async()=>{
  const directory=fixture('await new Promise(resolve=>setTimeout(resolve,20000));expect(value).toBe(true);');
  const {child,done}=start(directory);
  try {
    const deadline=Date.now()+15000;
    while(!existsSync(join(directory,"started"))&&Date.now()<deadline)await delay(20);
    assert(existsSync(join(directory,"started")),"Baseline test did not start");
    child.kill(signal);
    const result=await done;
    assert.equal(result.code,exit,result.stderr+result.stdout);
    const report=JSON.parse(result.stdout);assert.equal(report.runComplete,false);assert.equal(report.summary.score,null);
    assertClean(directory);
  } finally {if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
test("parent deadlines stop an uninstrumented synchronous loop, restart, and confirm the timeout",{timeout:60000},async()=>{
  const directory=fixture('if(!value){while(true){}}expect(value).toBe(true);');
  const sentinel=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore"});
  const {child,done}=start(directory,true);
  try {
    const result=await done;assert.equal(result.code,0,result.stderr+result.stdout);
    const report=JSON.parse(result.stdout);assert.equal(report.runComplete,true);
    assert.equal(report.mutants[0].status,"Timeout");assert.equal(report.mutants[0].attempts.length,2);
    assert.equal(sentinel.exitCode,null,"Unrelated process was terminated");
    assertClean(directory);
  } finally {sentinel.kill();if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
test("generator communication errors abort preparation instead of excluding candidates",{timeout:30000},async()=>{
  const directory=fixture('expect(value).toBe(true);');
  const {child,done}=start(directory,"corrupt");
  try{
    const result=await done;assert.equal(result.code,2,result.stderr+result.stdout);
    const report=JSON.parse(result.stdout);assert.equal(report.runComplete,false);
    assert(report.diagnostics.some(d=>d.code==="EC_MUTATION_PROTOCOL"));
    assert.equal(report.summary.ignored,0);assertClean(directory);
  }finally{if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
for(const [signal,exit] of [["SIGINT",130],["SIGTERM",143]])test(`${signal} reaps a live persistent generator during probe execution`,{timeout:30000},async()=>{
  const directory=fixture('if(existsSync(new URL("./generator.pid",import.meta.url))){writeFileSync(new URL("./probing",import.meta.url),"ready");await new Promise(resolve=>setTimeout(resolve,20000));}expect(value).toBe(true);');
  const {child,done}=start(directory,true);
  try{
    const deadline=Date.now()+15000;
    while(!existsSync(join(directory,"probing"))&&Date.now()<deadline)await delay(20);
    assert(existsSync(join(directory,"probing")),"Probe with persistent generator did not start");
    child.kill(signal);const result=await done;
    assert.equal(result.code,exit,result.stderr+result.stdout);
    assert.equal(JSON.parse(result.stdout).runComplete,false);assertClean(directory);
  }finally{if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});

for(const cache of [false,true])test(`one persistent generator handles multiple files with cache ${cache} and is reaped on completion`,{timeout:30000},async()=>{
  const directory=fixture('expect(value).toBe(true);');
  for(let i=0;i<10;i++)writeFileSync(join(directory,"src",`extra${i}.ts`),`export const extra${i}=true;`);
  const {child,done}=start(directory,true,cache?["--cache"]:[]);
  try {
    const result=await done;assert.equal(result.code,0,result.stderr+result.stdout);
    const report=JSON.parse(result.stdout);assert.equal(report.runComplete,true);
    assert.equal(report.mutants.length,11);
    assert.equal(readFileSync(join(directory,"generator.pid"),"utf8").trim().split("\n").length,1);
    assertClean(directory);
  } finally {if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
