import {temporaryProject} from "./support/project.mjs";
import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdirSync,writeFileSync,readFileSync,realpathSync,rmSync,cpSync,symlinkSync} from "node:fs";
import {resolve,dirname,join} from "node:path";
import {fileURLToPath} from "node:url";
import {runMutation,formatReport} from "../packages/core/index.mjs";
const repo=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
const dependencies=realpathSync(join(repo,"tests/fixtures/runners/node_modules"));
function project(t,runner){
  const root=temporaryProject(t,"errspec-runner-children-");

  cpSync(join(repo,"tests/fixtures/runners",runner),root,{recursive:true});
  mkdirSync(join(root,"node_modules"));
  const link=name=>{mkdirSync(dirname(join(root,"node_modules",name)),{recursive:true});symlinkSync(realpathSync(join(dependencies,name)),join(root,"node_modules",name),"dir");};
  if(runner==="jest"){link("jest");link("@jest/globals");link("jest-environment-node");}
  else{
    link("@playwright/test");link("vite");
    mkdirSync(join(root,"node_modules/@errspec"));cpSync(join(repo,"packages/playwright"),join(root,"node_modules/@errspec/playwright"),{recursive:true});
    writeFileSync(join(root,"playwright.config.mjs"),'export default {testDir:".",testMatch:"*.spec.mjs",projects:[{name:"browser",use:{browserName:"chromium",headless:true}}]};');
    writeFileSync(join(root,"index.html"),'<script type="module">import {observed} from "/src/inprocess.ts";globalThis.observed=observed;</script>');
  }
  rmSync(join(root,"src"),{recursive:true});mkdirSync(join(root,"src"));
  writeFileSync(join(root,"src/leaf.ts"),'export const value=2+3;');
  writeFileSync(join(root,"src/effect.ts"),'export const enabled=true;');
  writeFileSync(join(root,"src/bridge.ts"),'export {value} from "./leaf.ts";import "./effect.ts";');
  writeFileSync(join(root,"src/child.ts"),'import {value} from "./bridge.ts";if(value!==5)throw Error("wrong");');
  writeFileSync(join(root,"src/inprocess.ts"),'export const observed=true;');
  const header='import {spawn,spawnSync,execFileSync,execSync} from "node:child_process";';
  const body=`expect(spawnSync(process.execPath,["src/child.ts","--secret","PRIVATE_PAYLOAD_812"],{encoding:"utf8",env:{...process.env,SECRET_ARGUMENT:"SECRET_VALUE_812"}}).status).toBe(0);
expect(execFileSync(process.execPath,["src/child.ts"],{encoding:"utf8"})).toBe("");
expect(execSync(process.execPath+' src/child.ts',{encoding:"utf8"})).toBe("");
expect(await new Promise((resolve,reject)=>spawn(process.execPath,["src/child.ts"]).once("close",resolve).once("error",reject))).toBe(0);`;
  if(runner==="jest")writeFileSync(join(root,"value.test.ts"),`import {test,expect} from "@jest/globals";${header}import {observed} from "./src/inprocess.ts";test("node children",async()=>{${body}expect(observed).toBeDefined();});`);
  else writeFileSync(join(root,"value.spec.mjs"),`import {test,expect} from "@errspec/playwright";${header}test("node children",async({page})=>{${body}await page.goto("/");await page.waitForFunction(()=>typeof observed!=="undefined");expect(await page.evaluate(()=>observed)).toBeDefined();});`);
  return root;
}
for(const runner of ["jest","playwright"])test(`${runner} records actual Node child launches with shared source attribution and no raw arguments`,async t=>{
  const root=project(t,runner),transport={binary,cwd:root};
  const result=await runMutation({root,node:process.execPath,runner,policy:{timeBudget:0}},transport);
  assert.equal(result.exitCode,0,JSON.stringify(result));assert.equal(result.report.runComplete,true);
  const facts=result.report.childProcesses;assert.equal(facts.length,4,JSON.stringify(result));
  assert.deepEqual(new Set(facts.map(f=>f.method)),new Set(["spawn","spawnSync","execFileSync","execSync"]));
  assert.ok(facts.every(f=>f.test&&f.file===(runner==="jest"?"value.test.ts":"value.spec.mjs")&&f.entries.join(",")==="src/bridge.ts,src/child.ts,src/effect.ts,src/leaf.ts"),JSON.stringify(facts));
  assert.doesNotMatch(JSON.stringify(result.report),/PRIVATE_PAYLOAD_812|SECRET_VALUE_812|SECRET_ARGUMENT|"argv"|"env"/);
  const agent=JSON.parse(await formatReport(result.report,{reporter:"agent"},transport));
  assert.equal(agent.warnings.filter(w=>w.kind==="childProcess").length,4);
  assert.ok(agent.gaps.filter(g=>g.file==="src/leaf.ts").every(g=>g.childProcess&&g.fix.includes("child process")));
  const failed=await runMutation({root,node:process.execPath,runner,failOnUndetected:true,policy:{timeBudget:0}},transport);assert.equal(failed.exitCode,1,"observations cannot silently change the gate");
  const ordinary=runner==="jest"?'import {test,expect} from "@jest/globals";import {observed} from "./src/inprocess.ts";test("ordinary",()=>expect(observed).toBeDefined());':'import {test,expect} from "@errspec/playwright";test("ordinary",async({page})=>{await page.goto("/");await page.waitForFunction(()=>typeof observed!=="undefined");expect(await page.evaluate(()=>observed)).toBeDefined();});';
  writeFileSync(join(root,runner==="jest"?"value.test.ts":"value.spec.mjs"),ordinary);
  const clean=await runMutation({root,node:process.execPath,runner,policy:{timeBudget:0}},transport);assert.equal(clean.exitCode,0,JSON.stringify(clean));assert.equal(clean.report.childProcesses,undefined);
});
test("Jest environment setup and teardown launches are observed and restored between modules",async t=>{
  const root=project(t,"jest");
  writeFileSync(join(root,"src/setup.ts"),'import "./child.ts";');writeFileSync(join(root,"src/teardown.ts"),'import "./child.ts";');
  writeFileSync(join(root,"custom-env.cjs"),'const {TestEnvironment}=require("jest-environment-node");const cp=require("node:child_process");const original=cp.execFileSync;module.exports=class extends TestEnvironment{constructor(config,context){super(config,context);if(cp.execFileSync!==original)throw Error("observer leaked between environments");this.state={user:true};this.data={user:true};}async setup(){cp.execFileSync(process.execPath,["src/setup.ts"]);await super.setup();}async teardown(){if(!this.state.user||!this.data.user)throw Error("custom state overwritten");cp.execFileSync(process.execPath,["src/teardown.ts"]);await super.teardown();}};');
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"<rootDir>/custom-env.cjs",transform:{},testMatch:["**/*.test.ts"],transformIgnorePatterns:["/node_modules/","/custom-env.cjs$"]};');
  const source='import {test,expect} from "@jest/globals";import {observed} from "./src/inprocess.ts";test("ordinary",()=>expect(observed).toBeDefined());';
  writeFileSync(join(root,"value.test.ts"),source);writeFileSync(join(root,"other.test.ts"),source);
  const result=await runMutation({root,node:process.execPath,runner:"jest",policy:{timeBudget:0}},{binary,cwd:root});assert.equal(result.exitCode,0,JSON.stringify(result));
  assert.equal(result.report.childProcesses.length,4);
  assert.ok(result.report.childProcesses.every(f=>f.test===null&&f.method==="execFileSync"));
  assert.deepEqual(new Set(result.report.childProcesses.map(f=>f.file)),new Set(["value.test.ts","other.test.ts"]));
  writeFileSync(join(root,"custom-env.cjs"),readFileSync(join(root,"custom-env.cjs"),"utf8").replace('await super.teardown();','await super.teardown();throw Error("owned environment cleanup failed");'));
  const cleanup=await runMutation({root,node:process.execPath,runner:"jest",policy:{timeBudget:0}},{binary,cwd:root});
  assert.equal(cleanup.exitCode,2,JSON.stringify(cleanup));assert.match(cleanup.report.diagnostics[0].message,/owned environment cleanup failed/);assert.equal(cleanup.report.summary.killed,0);
});
test("Playwright rejects stale, unsupported or malformed fixture observations before publishing coverage",async t=>{
  const root=project(t,"playwright"),transport={binary,cwd:root};
  writeFileSync(join(root,"value.spec.mjs"),'import {test as base,expect} from "@errspec/playwright";const test=base.extend({_poison:[async({},use,info)=>{await use();await info.attach("errspec-mutation",{body:Buffer.from(JSON.stringify({nodeObserverVersion:1,runId:"another-execution",requestId:99,hits:{},infected:{}})),contentType:"application/json"});},{auto:true}]});test("stale",async({page})=>{await page.goto("/");await page.waitForFunction(()=>typeof observed!=="undefined");expect(await page.evaluate(()=>observed)).toBe(true);});');
  const stale=await runMutation({root,node:process.execPath,runner:"playwright"},transport);assert.equal(stale.exitCode,2,JSON.stringify(stale));assert.equal(stale.report.diagnostics[0].code,"EC_MUTATION_PROTOCOL");assert.equal(stale.report.summary.killed,0);
  writeFileSync(join(root,"value.spec.mjs"),'import {test as base,expect} from "@playwright/test";import {readFileSync} from "node:fs";const test=base.extend({_legacy:[async({},use,info)=>{await use();const state=JSON.parse(readFileSync(process.env.ERRSPEC_BROWSER_STATE,"utf8"));await info.attach("errspec-mutation",{body:Buffer.from(JSON.stringify({runId:state.context.runId,requestId:state.context.requestId,hits:{},infected:{}})),contentType:"application/json"});},{auto:true}]});test("old fixture",async({page})=>{await page.goto("/");await page.waitForFunction(()=>typeof observed!=="undefined");expect(await page.evaluate(()=>observed)).toBe(true);});');
  const old=await runMutation({root,node:process.execPath,runner:"playwright"},transport);assert.equal(old.exitCode,2,JSON.stringify(old));assert.match(old.report.diagnostics[0].message,/reinstall the matching/);assert.equal(old.report.summary.killed,0);
  writeFileSync(join(root,"value.spec.mjs"),readFileSync(join(root,"value.spec.mjs"),"utf8").replace('runId:state.context.runId,requestId:state.context.requestId,hits:{},infected:{}','nodeObserverVersion:1,runId:state.context.runId,requestId:state.context.requestId,hits:{}'));
  const malformed=await runMutation({root,node:process.execPath,runner:"playwright"},transport);assert.equal(malformed.exitCode,2,JSON.stringify(malformed));assert.equal(malformed.report.diagnostics[0].code,"EC_MUTATION_PROTOCOL");assert.equal(malformed.report.summary.killed,0);
});
test("Playwright observes application worker fixture setup and teardown without browser-launch noise",async t=>{
  const root=project(t,"playwright");
  writeFileSync(join(root,"src/setup.ts"),'import "./child.ts";');writeFileSync(join(root,"src/teardown.ts"),'import "./child.ts";');
  writeFileSync(join(root,"value.spec.mjs"),'import {test as base,expect} from "@errspec/playwright";import {execFileSync} from "node:child_process";const test=base.extend({resource:[async({},use)=>{execFileSync(process.execPath,["src/setup.ts"]);await use(true);execFileSync(process.execPath,["src/teardown.ts"]);},{scope:"worker"}]});test("worker resource",async({page,resource})=>{expect(resource).toBe(true);await page.goto("/");await page.waitForFunction(()=>typeof observed!=="undefined");expect(await page.evaluate(()=>observed)).toBe(true);});');
  const result=await runMutation({root,node:process.execPath,runner:"playwright",policy:{timeBudget:0}},{binary,cwd:root});assert.equal(result.exitCode,0,JSON.stringify(result));
  assert.equal(result.report.childProcesses.length,2,JSON.stringify(result));
  assert.ok(result.report.childProcesses.every(f=>f.method==="execFileSync"&&f.file==="value.spec.mjs"));
  assert.ok(result.report.childProcesses.some(f=>f.entries.includes("src/setup.ts")));assert.ok(result.report.childProcesses.some(f=>f.entries.includes("src/teardown.ts")));
});
