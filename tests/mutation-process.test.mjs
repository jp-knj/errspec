import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, readdirSync, existsSync, rmSync } from "node:fs";
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
  writeFileSync(join(directory,"x.test.ts"),'import {test,expect} from "vitest";import {writeFileSync} from "node:fs";import {value} from "./src/value";test("active",async()=>{writeFileSync(new URL("./started",import.meta.url),"ready");'+body+'});');
  symlinkSync(resolve(root,"examples/checkout/node_modules"),join(directory,"node_modules"),"dir");
  return directory;
}
function start(directory) {
  const child=spawn(binary,["mutate","--root",directory,"--node",process.execPath,"--format","json",...(process.env.ERRSPEC_REUSE_SESSION?["--reuse-session"]:[]),...families.flatMap(f=>["--exclude-mutator",f])],
    {env:{...process.env,TMPDIR:join(directory,"tmp")},stdio:["ignore","pipe","pipe"]});
  let stdout="",stderr="";child.stdout.on("data",c=>stdout+=c);child.stderr.on("data",c=>stderr+=c);
  const done=new Promise((resolve,reject)=>{child.on("error",reject);child.on("close",(code,signal)=>resolve({code,signal,stdout,stderr}));});
  return {child,done};
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
    assert.equal(readdirSync(join(directory,"tmp")).some(f=>f.startsWith("errspec-mutation-")),false);
  } finally {if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
test("parent deadlines stop an uninstrumented synchronous loop, restart, and confirm the timeout",{timeout:60000},async()=>{
  const directory=fixture('if(!value){while(true){}}expect(value).toBe(true);');
  const sentinel=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore"});
  const {child,done}=start(directory);
  try {
    const result=await done;assert.equal(result.code,0,result.stderr+result.stdout);
    const report=JSON.parse(result.stdout);assert.equal(report.runComplete,true);
    assert.equal(report.mutants[0].status,"Timeout");assert.equal(report.mutants[0].attempts.length,2);
    assert.equal(sentinel.exitCode,null,"Unrelated process was terminated");
    assert.equal(readdirSync(join(directory,"tmp")).some(f=>f.startsWith("errspec-mutation-")),false);
  } finally {sentinel.kill();if(child.exitCode===null)child.kill("SIGKILL");rmSync(directory,{recursive:true,force:true});}
});
