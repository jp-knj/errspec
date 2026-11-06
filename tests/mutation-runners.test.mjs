import {temporaryProject} from "./support/project.mjs";
import {runJsonCommand} from "./support/cli.mjs";
import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdirSync,writeFileSync,readFileSync,cpSync,existsSync,rmSync,symlinkSync} from "node:fs";
import {join,resolve,dirname} from "node:path";
import {fileURLToPath} from "node:url";
import {spawn,execFileSync} from "node:child_process";
import {once} from "node:events";
import {detectRunner} from "../packages/mutation-driver/runners.mjs";
import {runMutation} from "../packages/core/index.mjs";
const repo=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const dependencies=join(repo,"tests/fixtures/runners/node_modules");
const binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
function project(t,runner="jest"){
  const root=temporaryProject(t,"errspec-runner-");

  cpSync(join(repo,"tests/fixtures/runners",runner),root,{recursive:true});
  symlinkSync(dependencies,join(root,"node_modules"),"dir");
  return root;
}
function run(root,args=[],status=0){
  return runJsonCommand(binary,["mutate","--node",process.execPath,"--format","json",...args],{cwd:root,encoding:"utf8",timeout:120000},status);
}
test("runner detection follows config precedence and explicit selections",t=>{
  const root=project(t);assert.equal(detectRunner(root),"jest");
  writeFileSync(join(root,"playwright.config.ts"),"export default {};");assert.equal(detectRunner(root),"jest");
  writeFileSync(join(root,"vitest.config.ts"),"export default {};");assert.equal(detectRunner(root),"vitest");
  assert.equal(detectRunner(root,{runner:"playwright"}),"playwright");
  assert.equal(detectRunner(root,{vitestPackage:"/explicit"}),"vitest");
});
test("Jest 30 native ESM preserves native verdicts, TS, skipped tests and API identities",async t=>{
  const root=project(t),report=run(root,["--weak-mutation"]);
  assert.equal(report.engine.runner,"jest");assert.equal(report.engine.runnerVersion,"30.5.2");
  assert.equal(report.runComplete,true);assert.equal(report.baseline.filter(t=>t.started).length,1);
  assert.equal(report.baseline.find(t=>t.name==="skipped").started,false);
  assert.ok(report.summary.killed>0);assert.ok(report.summary.noCoverage>0);assert.equal(report.summary.runtimeError,0);
  const api=await runMutation({node:process.execPath,runner:"jest",weakMutation:true},{binary,cwd:root});
  assert.equal(api.exitCode,0);assert.deepEqual(api.report.mutants.map(m=>[m.key,m.status,m.coveredBy]),report.mutants.map(m=>[m.key,m.status,m.coveredBy]));
});
test("Jest fresh workers support parallel native planning and conservatively disable cache",t=>{
  const root=project(t),report=run(root,["--runner","jest","--cache","-j","2","--reuse-session"]);
  assert.equal(report.runComplete,true);assert.equal(report.cache.reused,0);assert.ok(report.cache.disabledReason);
  assert.equal(report.summary.runtimeError,0);assert.ok(report.summary.killed>0);
});
test("Jest baseline and unsupported CommonJS inputs fail with actionable diagnostics",t=>{
  const root=project(t);
  writeFileSync(join(root,"src/value.ts"),'export const checked=()=>99;');
  const failed=run(root,[],2);assert.equal(failed.diagnostics[0].code,"EC_MUTATION_BASELINE");assert.equal(failed.summary.killed,0);
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"commonjs"}));
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"node",transform:{},testMatch:["**/*.test.js"]};');
  writeFileSync(join(root,"value.test.js"),'const {test}=require("@jest/globals");test("commonjs",()=>{});');
  const unsupported=run(root,[],2);assert.equal(unsupported.diagnostics[0].code,"EC_MUTATION_BASELINE");assert.match(unsupported.diagnostics[0].message,/native ESM/);
});
test("Jest suite cleanup errors never become a passing baseline or assertion detection",t=>{
  const root=project(t);
  writeFileSync(join(root,"src/value.ts"),'export const checked=()=>true;');
  const source='import {test,expect,afterAll} from "@jest/globals";import {checked} from "./src/value.ts";test("checked",()=>{checked();expect(true).toBe(true)});';
  writeFileSync(join(root,"value.test.ts"),source+'afterAll(()=>{throw Error("suite cleanup failed")});');
  const baseline=run(root,["--runner","jest"],2);
  assert.equal(baseline.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.match(baseline.diagnostics[0].message,/suite cleanup failed/);
  assert.equal(baseline.summary.killed,0);
  writeFileSync(join(root,"value.test.ts"),source+'afterAll(()=>{if(checked()!==true)throw Error("mutant cleanup failed")});');
  const mutation=run(root,["--runner","jest"]);
  assert.equal(mutation.runComplete,true,JSON.stringify(mutation.diagnostics));
  assert.ok(mutation.summary.runtimeError>0,JSON.stringify(mutation.summary));
  assert.equal(mutation.summary.killed,0);
  assert.ok(mutation.mutants.filter(m=>m.status==="RuntimeError").every(m=>m.failedModules.includes("value.test.ts")&&m.killedBy.length===0&&m.attempts.length>1));
});
test("Jest rejects unsuccessful snapshot validation even when assertions passed",t=>{
  const root=project(t),snapshots=join(root,"__snapshots__");mkdirSync(snapshots);
  const snapshot=join(snapshots,"value.test.ts.snap"),source='// Jest Snapshot v1, https://jestjs.io/docs/snapshot-testing\n\nexports[`removed test 1`] = `"stale"`;\n';
  writeFileSync(snapshot,source);
  const report=run(root,["--runner","jest"],2);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.match(report.diagnostics[0].message,/snapshot/i);
  assert.ok(report.baseline.some(test=>test.started&&test.status==="passed"));
  assert.equal(report.summary.killed,0);
  assert.equal(readFileSync(snapshot,"utf8"),source);
});
for(const lifecycle of ["teardown","run_finish"])test(`Jest retains custom environment ${lifecycle} failures after passed assertions`,t=>{
  const root=project(t);
  const callback=lifecycle==="teardown"?'async teardown(){await super.teardown();throw Error("custom environment cleanup failed")}':'async handleTestEvent(event){if(event.name==="run_finish")throw Error("custom environment cleanup failed")}';
  writeFileSync(join(root,"environment.cjs"),'const {TestEnvironment}=require("jest-environment-node");module.exports=class extends TestEnvironment{'+callback+'};');
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"<rootDir>/environment.cjs",transform:{},testMatch:["**/*.test.ts"],transformIgnorePatterns:["/node_modules/","/environment.cjs$"]};');
  const report=run(root,["--runner","jest"],2);
  assert.equal(report.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.match(report.diagnostics[0].message,/custom environment cleanup failed/);
  assert.equal(report.summary.killed,0);
});
test("Jest preserves the project's asynchronous native ESM transform and its options",t=>{
  const root=project(t);
  writeFileSync(join(root,"src/value.ts"),'export const checked=(value:number)=>value+OFFSET;');
  writeFileSync(join(root,"transform.mjs"),'import {stripTypeScriptTypes} from "node:module";export default {createTransformer(options){return {async processAsync(code,file){return {code:stripTypeScriptTypes(code.replaceAll("OFFSET",String(options.offset)),{mode:"transform"})}}}}};');
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"node",testMatch:["**/*.test.ts"],transform:{"\\.ts$":["<rootDir>/transform.mjs",{offset:1}]}};');
  const report=run(root,["--runner","jest"]);assert.equal(report.runComplete,true);assert.ok(report.summary.killed>0);assert.equal(report.summary.runtimeError,0);
});
test("Vite+ uses its real bundled Vitest installation and existing native execution contract",t=>{
  const root=project(t,"vite-plus");
  rmSync(join(root,"node_modules"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(dependencies,"vite-plus"),join(root,"node_modules/vite-plus"),"dir");
  assert.equal(detectRunner(root),"vite-plus");
  const report=run(root,["--reuse-session"]);
  assert.equal(report.engine.runner,"vite-plus");assert.equal(report.engine.runnerVersion,"1.5.0");assert.equal(report.runComplete,true);
  assert.match(report.engine.vitest,/^5\./);assert.ok(report.summary.killed>0);assert.equal(report.summary.survived,0);
});

function browserProject(t,{projects=true,fixture=true}={}){
  const root=project(t,"playwright");
  if(!projects)writeFileSync(join(root,"playwright.config.mjs"),'export default {testDir:".",testMatch:"*.spec.mjs",use:{headless:true}};');
  if(!fixture)writeFileSync(join(root,"value.spec.mjs"),readFileSync(join(root,"value.spec.mjs"),"utf8").replace('@errspec/playwright','@playwright/test'));
  return root;
}
test("Playwright + Vite uses real browser fixtures and distinct configured project identities",t=>{
  const root=browserProject(t),report=run(root,["--runner","playwright","--weak-mutation"]);
  assert.equal(report.runComplete,true);assert.equal(report.engine.runner,"playwright");
  assert.deepEqual(report.baseline.filter(t=>t.started).map(t=>JSON.parse(t.id)[0]),["first","second"]);
  assert.equal(report.baseline.filter(t=>!t.started).length,2);
  assert.ok(report.summary.killed>0);assert.ok(report.summary.noCoverage>0);assert.equal(report.summary.runtimeError,0);
  assert.ok(report.mutants.filter(m=>m.status==="Killed").every(m=>m.coveredBy.length===2));
});
test("Playwright requires observed fixture metadata and rejects unsupported custom app servers",t=>{
  const root=browserProject(t,{projects:false,fixture:false});
  const missing=run(root,["--runner","playwright"],2);assert.match(missing.diagnostics[0].message,/fixture metadata/);assert.equal(missing.summary.killed,0);
  writeFileSync(join(root,"playwright.config.mjs"),'export default {webServer:{command:"node custom-server.mjs",url:"http://localhost:3000"}};');
  const invalid=run(root,["--runner","playwright"],2);assert.match(invalid.diagnostics[0].message,/Playwright failed before|Vite app/);
});
test("Playwright extended fixtures retain hits from previous documents and isolated parallel workers",t=>{
  const root=browserProject(t);
  writeFileSync(join(root,"blank.html"),"<p>next document</p>");
  writeFileSync(join(root,"frame.html"),readFileSync(join(root,"index.html")));
  writeFileSync(join(root,"value.spec.mjs"),`import {test as base,expect} from "@playwright/test";import {mutationFixtures} from "@errspec/playwright";
const test=base.extend({answer:async({},use)=>use(3)}).extend(mutationFixtures);
test("previous document",async({page,answer})=>{await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");expect(await page.evaluate(()=>checked(2))).toBe(answer);await page.goto("/blank.html");expect(await page.locator("p").textContent()).toBe("next document");});
test("iframe",async({page,answer})=>{await page.goto("/blank.html");await page.evaluate(()=>{const frame=document.createElement("iframe");frame.src="/frame.html";document.body.append(frame)});const frame=await page.waitForEvent("framenavigated");await frame.waitForFunction(()=>typeof checked==="function");expect(await frame.evaluate(()=>checked(2))).toBe(answer);});`);
  const report=run(root,["--runner","playwright","--runner-project","second","--weak-mutation","--cache","-j","2"]);
  assert.equal(report.runComplete,true);assert.equal(report.cache.reused,0);assert.ok(report.cache.disabledReason);
  assert.ok(report.summary.killed>0);assert.equal(report.summary.runtimeError,0);
  assert.deepEqual(report.baseline.map(t=>JSON.parse(t.id)[0]),["second","second"]);
  assert.ok(report.mutants.filter(m=>m.status==="Killed").every(m=>m.coveredBy.length===2));
});

const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function processTree(){
  return execFileSync("ps",["-axo","pid=,ppid=,state=,command="],{encoding:"utf8"}).split("\n").flatMap(line=>{
    const match=line.match(/^\s*(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/);
    return match?[{pid:Number(match[1]),parent:Number(match[2]),state:match[3],command:match[4]}]:[];
  });
}
for(const signal of ["SIGINT","SIGKILL"])test(`Playwright ${signal} closes owned Vite, worker and browser descendants`,async t=>{
  const root=browserProject(t,{projects:false}),ready=join(root,"browser-ready.json");
  writeFileSync(join(root,"value.spec.mjs"),`import {test} from "@errspec/playwright";import {writeFileSync} from "node:fs";
test("keep browser alive",async({page})=>{await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");writeFileSync(${JSON.stringify(ready)},JSON.stringify({worker:process.pid}));await new Promise(()=>{});});`);
  const child=spawn(binary,["mutate","--node",process.execPath,"--format","json","--runner","playwright"],{cwd:root,stdio:["ignore","pipe","pipe"]});
  let stderr="",stdout="";child.stderr.on("data",chunk=>stderr+=chunk);child.stdout.on("data",chunk=>stdout+=chunk);
  const exited=once(child,"exit");let descendants=[];
  t.after(()=>{
    if(child.exitCode===null&&child.signalCode===null)child.kill("SIGKILL");
    const live=processTree();for(const process of descendants)if(live.some(p=>p.pid===process.pid&&p.command===process.command&&!p.state.startsWith("Z"))){try{globalThis.process.kill(process.pid,"SIGKILL");}catch{}}
  });
  for(let n=0;n<600&&!existsSync(ready)&&child.exitCode===null&&child.signalCode===null;n++)await pause(50);
  assert.ok(existsSync(ready),stderr+stdout);
  const snapshot=processTree(),owned=new Set([child.pid]);
  for(let changed=true;changed;){changed=false;for(const process of snapshot)if(owned.has(process.parent)&&!owned.has(process.pid)){owned.add(process.pid);changed=true;}}
  descendants=snapshot.filter(p=>owned.has(p.pid)&&p.pid!==child.pid);
  assert.ok(descendants.some(p=>/chrom(?:e|ium)|headless_shell/i.test(p.command)),"must observe a real browser descendant");
  child.kill(signal);const [code,terminated]=await exited;
  if(signal==="SIGINT"){assert.equal(code,130,stderr+stdout);assert.equal(JSON.parse(stdout).runComplete,false);}else assert.equal(terminated,"SIGKILL");
  let remaining;
  for(let n=0;n<200;n++){
    const live=processTree();remaining=descendants.filter(process=>live.some(p=>p.pid===process.pid&&p.command===process.command&&!p.state.startsWith("Z")));
    if(!remaining.length)break;await pause(50);
  }
  assert.deepEqual(remaining,[],"tool-launched browser/server descendants must close after native interruption");
});

test("native ESM and browser baselines never create or update project snapshots",t=>{
  const jest=project(t);
  writeFileSync(join(jest,"value.test.ts"),'import {test,expect} from "@jest/globals";test("snapshot",()=>expect("stable").toMatchSnapshot());');
  const jestReport=run(jest,["--runner","jest"],2);assert.equal(jestReport.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.equal(existsSync(join(jest,"__snapshots__")),false);
  const browser=browserProject(t,{projects:false});
  writeFileSync(join(browser,"playwright.config.mjs"),'export default {testDir:".",testMatch:"*.spec.mjs",updateSnapshots:"all",use:{headless:true}};');
  writeFileSync(join(browser,"value.spec.mjs"),'import {test,expect} from "@errspec/playwright";test("snapshot",async({page})=>{await page.goto("/");expect("stable").toMatchSnapshot("value.txt");});');
  const browserReport=run(browser,["--runner","playwright"],2);assert.equal(browserReport.diagnostics[0].code,"EC_MUTATION_BASELINE");
  assert.equal(existsSync(join(browser,"value.spec.mjs-snapshots")),false);
});
test("targets excluded from Jest transforms produce an unsupported-input diagnostic",t=>{
  const root=project(t);
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"node",transform:{},transformIgnorePatterns:["<rootDir>/src/"],testMatch:["**/*.test.ts"]};');
  const report=run(root,["--runner","jest"],2);assert.equal(report.diagnostics[0].code,"EC_MUTATION_CONFIG");
  assert.match(report.diagnostics[0].message,/excluded from Jest transforms/);assert.equal(report.summary.killed,0);
});
test("Jest plans exact duplicate test identities and skips unrelated tests during trials",t=>{
  const root=project(t),marker=join(root,"unrelated-runs.txt");
  writeFileSync(join(root,"value.test.ts"),`import {test,expect} from "@jest/globals";import {appendFileSync} from "node:fs";import {checked} from "./src/value.ts";test("duplicate",()=>expect(checked(2)).toBe(3));test("duplicate",()=>expect(checked(0)).toBe(1));test("unrelated",()=>appendFileSync(${JSON.stringify(marker)},"x"));`);
  const report=run(root,["--runner","jest"]);assert.equal(report.runComplete,true);
  assert.deepEqual(report.baseline.filter(t=>t.name==="duplicate").map(t=>JSON.parse(t.id)[3]),[0,1]);
  assert.equal(readFileSync(marker,"utf8"),"xx","unrelated test runs only in plain/probe baselines");
  assert.ok(report.mutants.filter(m=>m.status==="Killed").every(m=>m.coveredBy.length===2));
});
test("Jest concurrent test coverage is conservatively attributed to the complete module cohort",t=>{
  const root=project(t);
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "@jest/globals";import {checked} from "./src/value.ts";test.concurrent("first",async()=>{await Promise.resolve();expect(checked(2)).toBe(3)});test.concurrent("second",async()=>{await Promise.resolve();expect(checked(0)).toBe(1)});test("unrelated",()=>{});');
  const report=run(root,["--runner","jest","--weak-mutation"]);assert.equal(report.runComplete,true);assert.equal(report.summary.runtimeError,0);
  assert.ok(report.mutants.filter(m=>m.status==="Killed").every(m=>m.coveredBy.length===3));
});

test("Playwright expected failures preserve the project's successful baseline semantics",t=>{
  const root=browserProject(t,{projects:false});
  writeFileSync(join(root,"src/value.ts"),'export const checked=()=>true;');
  writeFileSync(join(root,"value.spec.mjs"),'import {test,expect} from "@errspec/playwright";test("expected failure",async({page})=>{test.fail();await page.goto("/");await page.waitForFunction(()=>typeof checked==="function");expect(await page.evaluate(()=>checked())).toBe(false);});');
  const report=run(root,["--runner","playwright"]);assert.equal(report.runComplete,true);assert.equal(report.baseline[0].status,"passed");
  assert.ok(report.mutants.some(m=>m.mutator==="BooleanLiteral"&&m.status==="Killed"));
});
for(const runner of ['jest','playwright'])test(`${runner} custom plugin acquisition shares native generation and excludes plugin modules`,t=>{
  const root=runner==='playwright'?browserProject(t,{projects:false}):project(t);
  writeFileSync(join(root,'src/custom-plugin.mjs'),"export default {name:'ConfiguredNumber',cache:'deterministic',visit(node){if(node.kind==='NumericLiteral'&&node.text==='1')return [{replacement:'2'}]}};");
  const result=run(root,['--runner',runner,'--plugin','./src/custom-plugin.mjs']);
  assert.equal(result.runComplete,true);
  const custom=result.mutants.filter(m=>m.mutator==='ConfiguredNumber');assert.equal(custom.length,1);assert.equal(custom[0].status,'Killed');
  assert.ok(result.mutants.every(m=>m.file!=='src/custom-plugin.mjs'));
});
test("runner scratch caches stay in the owned worker workspace",t=>{
  const jest=project(t);
  writeFileSync(join(jest,"jest.config.mjs"),'export default {testEnvironment:"node",transform:{},testMatch:["**/*.test.ts"],cacheDirectory:"<rootDir>/user-cache"};');
  assert.equal(run(jest,["--runner","jest"]).runComplete,true);
  assert.equal(existsSync(join(jest,"user-cache")),false);
  const browser=browserProject(t,{projects:false});
  writeFileSync(join(browser,"vite.config.mjs"),'export default {cacheDir:"./user-cache"};');
  assert.equal(run(browser,["--runner","playwright"]).runComplete,true);
  assert.equal(existsSync(join(browser,"user-cache")),false);
});

test("Jest metadata supports deeply nested real test paths",t=>{
  const root=project(t),directory=join(root,"tests","a".repeat(80),"b".repeat(80));
  mkdirSync(directory,{recursive:true});rmSync(join(root,"value.test.ts"));
  writeFileSync(join(directory,"value.test.ts"),'import {test,expect} from "@jest/globals";import {checked} from "../../../src/value.ts";test("deep",()=>expect(checked(2)).toBe(3));');
  const report=run(root,["--runner","jest"]);assert.equal(report.runComplete,true);assert.ok(report.summary.killed>0);
  assert.ok(report.baseline[0].file.length>160);
});
test("Jest rejects worker metadata from another execution before assigning verdicts",t=>{
  const root=project(t);
  writeFileSync(join(root,"custom-env.cjs"),'const {TestEnvironment}=require("jest-environment-node");const {readFileSync,writeFileSync,readdirSync}=require("node:fs");const {join}=require("node:path");module.exports=class extends TestEnvironment{async teardown(){process.once("exit",()=>{const state=JSON.parse(readFileSync(process.env.ERRSPEC_JEST_STATE,"utf8"));const file=join(state.output,readdirSync(state.output).find(name=>name.endsWith(".json")));const data=JSON.parse(readFileSync(file,"utf8"));data.runId="another-execution";writeFileSync(file,JSON.stringify(data));});await super.teardown();}};');
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"<rootDir>/custom-env.cjs",transform:{},testMatch:["**/*.test.ts"],transformIgnorePatterns:["/node_modules/","/custom-env.cjs$"]};');
  const report=run(root,["--runner","jest"],2);assert.equal(report.diagnostics[0].code,"EC_MUTATION_PROTOCOL");assert.match(report.diagnostics[0].message,/Stale Jest worker metadata/);assert.equal(report.summary.killed,0);
});

test("Jest acquisition preserves custom environment state and reports all-skipped baselines honestly",t=>{
  const root=project(t);
  writeFileSync(join(root,"custom-env.cjs"),'const {TestEnvironment}=require("jest-environment-node");module.exports=class extends TestEnvironment{constructor(config,context){super(config,context);this.state={user:true};this.data={user:true};this.file="user-owned";}async setup(){if(!this.state.user||!this.data.user||this.file!=="user-owned")throw Error("custom state overwritten");await super.setup();}async teardown(){if(!this.state.user||!this.data.user||this.file!=="user-owned")throw Error("custom state overwritten");await super.teardown();}};');
  writeFileSync(join(root,"jest.config.mjs"),'export default {testEnvironment:"<rootDir>/custom-env.cjs",transform:{},testMatch:["**/*.test.ts"],transformIgnorePatterns:["/node_modules/","/custom-env.cjs$"]};');
  const report=run(root,["--runner","jest"]);assert.equal(report.runComplete,true);assert.ok(report.summary.killed>0);
  writeFileSync(join(root,"value.test.ts"),'import {test} from "@jest/globals";test.skip("skipped",()=>{});');
  const skipped=run(root,["--runner","jest"],2);assert.equal(skipped.testsExecuted,false);assert.equal(skipped.diagnostics[0].code,"EC_MUTATION_BASELINE");
});
