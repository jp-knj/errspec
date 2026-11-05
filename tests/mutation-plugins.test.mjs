import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, symlinkSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { runInNewContext } from "node:vm";
import { defineMutator, defineIgnorer, definePlugin, runMutation, formatReport } from "../packages/core/index.mjs";
import { loadPlugins } from "../packages/mutation-driver/plugins.mjs";
import { instrument } from "../packages/mutation-driver/emitter.mjs";
const repo=fileURLToPath(new URL("../",import.meta.url)), binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
const sdk=pathToFileURL(join(repo,"packages/core/index.mjs")).href;
function fixture(t) {
  const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-plugins-")));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,"src"));mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"module",dependencies:{vitest:"5.0.5"}}));
  writeFileSync(join(root,"src/value.ts"),"export const value=()=>2;");
  writeFileSync(join(root,"value.test.ts"),'import {test,expect} from "vitest";import {value} from "./src/value";test("value",()=>expect(value()).toBe(2));');
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["value.test.ts"]}};');
  return root;
}
function native(source,extra={}) {
  const output=spawnSync(binary,["__mutation-source"],{input:JSON.stringify({file:"src/value.ts",source,facts:true,...extra}),encoding:"utf8"});
  assert.equal(output.status,0,output.stderr);return JSON.parse(output.stdout);
}
function cli(root,args=[],code=0) {
  const output=spawnSync(binary,["mutate","--node",process.execPath,"--format","json","--reuse-session",...args],{cwd:root,encoding:"utf8",timeout:90000});
  assert.ifError(output.error);assert.equal(output.status,code,output.stdout+output.stderr);return JSON.parse(output.stdout);
}
const pluginSource=(replacement="3",extra="")=>`import {defineMutator,definePlugin} from ${JSON.stringify(sdk)};
export default definePlugin({name:'numbers',cache:'deterministic',mutators:[defineMutator({name:'CustomNumber',visit(node,ctx){if(node.kind==='NumericLiteral')return [{replacement:${JSON.stringify(replacement)}}]}})]${extra}});`;
test("public definitions preserve callbacks and owned native identities, UTF8 edits and receiver semantics",async t=>{
  const root=fixture(t),source='const label="日本🧪"; const receiver={n:2,get(){return this.n;}};globalThis.answer=[label,receiver.get()];';
  const plugin=definePlugin({name:"owned",mutators:[defineMutator({name:"CustomNumber",visit(node){if(node.kind==="NumericLiteral")return [{replacement:"3"}];}})],ignorers:[defineIgnorer({name:"unused",shouldIgnore(){}})]});
  assert.equal(typeof plugin.mutators[0].visit,"function");
  writeFileSync(join(root,"plugin.mjs"),`export default {name:'owned',cache:'deterministic',mutators:[{name:'CustomNumber',visit(node){if(node.kind==='NumericLiteral')return [{replacement:'3'}]}},{name:'PreserveReceiver',visit(node,ctx){if(node.kind==='PropertyAccessExpression'&&node.text==='receiver.get')return [{replacement:'(receiver.get)'}]}}]};`);
  const plugins=await loadPlugins(["./plugin.mjs"],root,()=>{}),base=native(source);
  const edits=await plugins.edits(source,"src/value.ts",base.metadata),generated=native(source,{customMutators:plugins.names,...edits});
  assert.deepEqual(generated.candidates.filter(c=>!plugins.names.includes(c.mutator)).map(c=>c.key),base.candidates.map(c=>c.key));
  const custom=generated.candidates.filter(c=>plugins.names.includes(c.mutator));assert.equal(custom.length,2);
  assert.equal(custom.find(c=>c.mutator==='PreserveReceiver').original,'receiver.get()');
  const code=instrument(source,generated.candidates,"src/value.ts","plugins").code;
  for(const candidate of custom) {
    const bytes=Buffer.from(source),direct=Buffer.concat([bytes.subarray(0,candidate.byteOffset),Buffer.from(candidate.replacement),bytes.subarray(candidate.end)]).toString();
    const execute=(script,key)=>{const context={[Symbol.for("errspec.mutation.plugins")]:id=>id===key};runInNewContext(script,context);return JSON.stringify(context.answer);};
    assert.equal(execute(code,candidate.key),execute(direct,null));
  }
  const shifted=native("\n\n"+source,{customMutators:plugins.names,...await plugins.edits("\n\n"+source,"src/value.ts",native("\n\n"+source).metadata)});
  assert.deepEqual(shifted.candidates.map(c=>c.key),generated.candidates.map(c=>c.key));
});
test("TS and CommonJS modules, arrays, package dependencies and declared inputs are acquired",async t=>{
  const root=fixture(t),files=new Set();
  writeFileSync(join(root,"plugin-data.txt"),"policy");
  writeFileSync(join(root,"dependency.cjs"),"module.exports='constant policy';");
  writeFileSync(join(root,"ignore.cjs"),"const reason=require('./dependency.cjs');module.exports={name:'CommonIgnore',cache:'deterministic',inputs:['plugin-data.txt'],shouldIgnore(node){if(node.kind==='NumericLiteral')return reason}};");
  writeFileSync(join(root,"plugin.ts"),"export default [{name:'TypeScriptNumber',cache:'deterministic',visit(node:{kind:string}){if(node.kind==='NumericLiteral')return [{replacement:'3'}]}}];");
  const plugins=await loadPlugins(["./ignore.cjs","./plugin.ts"],root,file=>files.add(file));
  assert.deepEqual(plugins.names,["TypeScriptNumber"]);assert.equal(plugins.cacheable,true);
  for(const file of ["ignore.cjs","dependency.cjs","plugin-data.txt","plugin.ts","package.json"])assert.ok(files.has(join(root,file)),file);
  const source="const value=2;",edits=await plugins.edits(source,"src/value.ts",native(source).metadata);
  assert.equal(edits.pluginEdits.length,1);assert.equal(edits.pluginIgnores[0].reason,"CommonIgnore: constant policy");
});
test("custom body fallthrough, condition statements and labeled loops match direct replacement",async t=>{
  const root=fixture(t),source="function body(){globalThis.events.push('original');return 7;} function choose(n){if(n>0)return 1;return 0;} let ticks=0; label:for(let i=0;i<2;i++){ticks++;continue label;}globalThis.answer=[body(),choose(1),ticks];";
  writeFileSync(join(root,"plugin.mjs"),`export default {name:'Statements',cache:'deterministic',mutators:[
    {name:'Body',visit(n,ctx){if(n.kind==='Block'&&ctx.parent.name==='body')return [{replacement:"{globalThis.events.push('replacement');}"}]}},
    {name:'If',visit(n){if(n.kind==='IfStatement')return [{replacement:';'}]}},
    {name:'Loop',visit(n){if(n.kind==='ForStatement')return [{replacement:'for(let i=0;i<1;i++){ticks++;continue label;}'}]}}
  ]};`);
  const plugins=await loadPlugins(["./plugin.mjs"],root,()=>{}),base=native(source);
  const generated=native(source,{customMutators:plugins.names,...await plugins.edits(source,'src/value.ts',base.metadata)});
  const custom=generated.candidates.filter(c=>plugins.names.includes(c.mutator));assert.equal(custom.length,3);assert.ok(custom.every(c=>!c.ignored));
  const code=instrument(source,generated.candidates,'src/value.ts','statements').code;
  native(code,{validateOnly:true});
  function execute(code,key){const context={events:[],[Symbol.for('errspec.mutation.statements')]:id=>id===key};runInNewContext(code,context);return JSON.stringify([context.answer,context.events]);}
  for(const candidate of custom){const bytes=Buffer.from(source),direct=Buffer.concat([bytes.subarray(0,candidate.byteOffset),Buffer.from(candidate.replacement),bytes.subarray(candidate.end)]).toString();assert.equal(execute(code,candidate.key),execute(direct,null),candidate.mutator);}
});
test("module/package plugins execute through CLI and API; named exclusions and comment/arid precedence stay native",async t=>{
  const root=fixture(t);
  mkdirSync(join(root,"node_modules/custom-plugin"));
  writeFileSync(join(root,"node_modules/custom-plugin/package.json"),JSON.stringify({name:"custom-plugin",type:"module",exports:"./index.mjs"}));
  writeFileSync(join(root,"node_modules/custom-plugin/index.mjs"),pluginSource());
  const plain=cli(root),command=cli(root,["--plugin","custom-plugin","-j","2"]);
  assert.equal(command.mutants.filter(m=>m.mutator==="CustomNumber").length,1);
  assert.equal(command.mutants.find(m=>m.mutator==="CustomNumber").status,"Killed");
  assert.deepEqual(command.mutants.filter(m=>m.mutator!=="CustomNumber").map(m=>m.key),plain.mutants.map(m=>m.key));
  const api=await runMutation({node:process.execPath,plugins:["custom-plugin"],reuseSession:true},{binary,cwd:root});
  assert.equal(api.exitCode,0);assert.deepEqual(api.report.mutants.map(m=>[m.key,m.status]),command.mutants.map(m=>[m.key,m.status]));
  writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({plugins:['custom-plugin']}));
  assert.ok(cli(root).mutants.some(m=>m.mutator==='CustomNumber'));
  const cleared=await runMutation({node:process.execPath,plugins:[],reuseSession:true},{binary,cwd:root});
  assert.equal(cleared.exitCode,0);assert.deepEqual(cleared.report.mutants.map(m=>m.key),plain.mutants.map(m=>m.key));
  assert.deepEqual(cli(root,["--plugin","custom-plugin","--exclude-mutator","CustomNumber"]).mutants.map(m=>m.key),plain.mutants.map(m=>m.key));
  writeFileSync(join(root,"src/value.ts"),"// errspec-disable-next-line CustomNumber: configured constant\nexport const value=()=>2;\nexport function telemetry(){metrics.emit(2)}");
  writeFileSync(join(root,"ignore.mjs"),`import {definePlugin,defineMutator,defineIgnorer} from ${JSON.stringify(sdk)};export default definePlugin({name:'rules',cache:'deterministic',aridCallees:['metrics.*'],mutators:[defineMutator({name:'CustomNumber',visit(n){if(n.kind==='NumericLiteral')return [{replacement:'3'}]}})],ignorers:[defineIgnorer({name:'numeric',shouldIgnore(n){if(n.kind==='NumericLiteral')return 'constant policy'}})]});`);
  const controls=cli(root,["--plugin","./ignore.mjs","--arid"]),numbers=controls.mutants.filter(m=>m.mutator==="CustomNumber");
  assert.deepEqual(numbers.map(m=>m.ignored),["disabled: configured constant","arid: logging-only code"]);assert.ok(numbers.every(m=>m.status==="Ignored"&&m.attempts.length===0));
  const ignored=cli(root,["--plugin","./ignore.mjs"]);
  assert.equal(ignored.mutants.filter(m=>m.mutator==="CustomNumber")[1].ignored,"numeric: constant policy");
});
test("deterministic plugin closure, input data and settings invalidate real cached trials",async t=>{
  const root=fixture(t),outer=realpathSync(mkdtempSync(join(tmpdir(),"errspec-plugin-closure-")));t.after(()=>rmSync(outer,{recursive:true,force:true}));
  writeFileSync(join(outer,"helper.mjs"),'export const numeric="3";');writeFileSync(join(outer,"data.txt"),"3");
  writeFileSync(join(outer,"plugin.mjs"),`import {numeric} from './helper.mjs';export default {name:'CachePlugin',cache:'deterministic',mutators:[{name:'CustomNumber',visit(n,ctx){if(n.kind==='NumericLiteral'){ctx.readInput('data.txt');return [{replacement:ctx.settings?.replacement??numeric}]}}}]};`);
  const spec={module:join(outer,"plugin.mjs"),settings:{replacement:"3"}};
  const run=async()=>{const result=await runMutation({node:process.execPath,plugins:[spec],cache:true,reuseSession:true},{binary,cwd:root});assert.equal(result.exitCode,0,JSON.stringify(result));return result.report;};
  const cold=await run(),warm=await run();assert.ok(cold.cache.executed>0);assert.equal(warm.cache.executed,0);assert.ok(warm.cache.reused>0);assert.equal(warm.cache.disabledReason,undefined);
  for(const [file,content] of [["helper.mjs",'export const numeric="4";'],["data.txt","4"],["plugin.mjs",readFileSync(join(outer,"plugin.mjs"),"utf8")+"\n// changed plugin implementation"]]) {
    writeFileSync(join(outer,file),content);const changed=await run();assert.equal(changed.cache.reused,0);assert.ok(changed.cache.executed>0);assert.equal(changed.cache.invalidated,true);
  }
  spec.settings.replacement="5";const settings=await run();assert.equal(settings.cache.reused,0);assert.ok(settings.cache.executed>0);
  const cached=await run();assert.ok(cached.cache.reused>0);
  writeFileSync(join(outer,"plugin.mjs"),readFileSync(join(outer,"plugin.mjs"),"utf8").replace("cache:'deterministic',",""));
  const unknown=await run();assert.equal(unknown.cache.reused,0);assert.match(unknown.cache.disabledReason,/cannot be fingerprinted/);
});
test("malformed ranges, syntax, duplicate names and callback errors fail with named diagnostics",t=>{
  const root=fixture(t);
  const bad=[
    ["Range", "if(n.kind==='NumericLiteral')return [{replacement:'3',range:{start:0,end:1}}]", /Range.*invalid anchor or range/],
    ["Syntax", "if(n.kind==='NumericLiteral')return [{replacement:'1 +'}]", /Syntax.*invalid syntax/],
    ["Throws", "throw Error('visitor boom')", /Throws.*visitor boom/],
    ["StringLiteral", "", /duplicates a built-in/],
  ];
  for(const [name,body,expected] of bad) {
    writeFileSync(join(root,"plugin.mjs"),`export default {name:${JSON.stringify(name)},visit(n){${body}}};`);
    const failed=cli(root,["--plugin","./plugin.mjs"],2);assert.match(failed.diagnostics[0].message,expected);assert.equal(failed.runComplete,false);
  }
  writeFileSync(join(root,"plugin.mjs"),"export default {name:'IgnoreThrows',shouldIgnore(){throw Error('ignorer boom')}};");
  assert.match(cli(root,["--plugin","./plugin.mjs"],2).diagnostics[0].message,/IgnoreThrows.*ignorer boom/);
  assert.match(cli(root,["--plugin","./missing.mjs"],2).diagnostics[0].message,/Cannot load mutation plugin/);
});
test("dynamic plugin inputs are acquired before baseline tests and cannot change during the run",t=>{
  const root=fixture(t);
  writeFileSync(join(root,"data.txt"),"3");
  writeFileSync(join(root,"plugin.mjs"),"export default {name:'Input',cache:'deterministic',visit(n,ctx){if(n.kind==='NumericLiteral')return [{replacement:ctx.readInput('data.txt')}]}};");
  writeFileSync(join(root,"value.test.ts"),`import {test,expect} from 'vitest';import {writeFileSync} from 'node:fs';import {value} from './src/value';test('value',()=>{expect(value()).toBe(2);writeFileSync(${JSON.stringify(join(root,"data.txt"))},'4')});`);
  const changed=cli(root,["--cache","--plugin","./plugin.mjs"],2);
  assert.equal(changed.runComplete,false);assert.equal(changed.diagnostics[0].code,"EC_MUTATION_INPUT");assert.match(changed.diagnostics[0].message,/data\.txt/);
});
test("plugins generate once per source with static imports and format through the shared reporter",async t=>{
  const root=fixture(t),audit=realpathSync(mkdtempSync(join(tmpdir(),'errspec-plugin-audit-'))),log=join(audit,'visits.log');
  t.after(()=>rmSync(audit,{recursive:true,force:true}));
  writeFileSync(join(root,'plugin.mjs'),`import {appendFileSync} from 'node:fs';export default {name:'Number',cache:'deterministic',visit(n){if(n.kind==='NumericLiteral'){appendFileSync(${JSON.stringify(log)},'visit\\n');return [{replacement:'3'}]}}};`);
  const options={node:process.execPath,plugins:['./plugin.mjs'],cache:true,cacheScope:true,experimentalCallgraph:true,reuseSession:true};
  const cold=await runMutation(options,{binary,cwd:root}),warm=await runMutation(options,{binary,cwd:root});
  for(const result of [cold,warm])assert.equal(result.exitCode,0,JSON.stringify(result));
  assert.equal(readFileSync(log,'utf8'),'visit\nvisit\n','static import acquisition must consume the generated plugin result');
  assert.equal(warm.report.cache.executed,0,JSON.stringify(warm.report.cache));assert.ok(warm.report.cache.reused>0);
  assert.equal(warm.report.mutants.filter(m=>m.mutator==='Number').length,1);
  assert.deepEqual(warm.report.mutants.map(m=>[m.key,m.status]),cold.report.mutants.map(m=>[m.key,m.status]));
  const agent=JSON.parse(await formatReport(warm.report,{reporter:'agent'},{binary,cwd:root}));
  assert.equal(agent.kind,'errspec/mutation-agent');
});
