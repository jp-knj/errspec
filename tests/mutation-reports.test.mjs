import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
import vm from 'node:vm';
import {formatReport,runMutation} from '../packages/core/index.mjs';
const require=createRequire(new URL('../packages/report-tests/package.json',import.meta.url));
const Ajv=require('ajv');
const schema=JSON.parse(readFileSync(new URL('../packages/core/schema/mutation-testing-report.schema.json',import.meta.url),'utf8'));
const validate=new Ajv({strict:false,validateFormats:false}).compile(schema);
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=join(repo,'target/debug/errspec');
function project(t) {
  const root=realpathSync(mkdtempSync(join(tmpdir(),'errspec-reports-')));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  mkdirSync(join(root,'src'));mkdirSync(join(root,'node_modules'));
  symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(root,'node_modules/vitest'),'dir');
  writeFileSync(join(root,'src/value.ts'),'export function value(n:number){ console.log("observation",n); return n > 0; }\nexport const unseen=()=>false;\n');
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {value} from "./src/value";test("value",()=>expect(value(2)).toBeDefined());');
  writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');
  return root;
}
function cli(root,args=[],code=0,env={}) {
  const result=spawnSync(binary,['mutate','--node',process.execPath,...args],{cwd:root,encoding:'utf8',timeout:60000,env:{...process.env,...env}});
  assert.ifError(result.error);assert.equal(result.status,code,result.stdout+result.stderr);return result;
}
const transport=root=>({binary,cwd:root});
test('real native results render MTE schema, function explanations, simplify candidates, and offline HTML',async t=>{
  const root=project(t),result=await runMutation({node:process.execPath,weakMutation:true},{binary,cwd:root});
  assert.equal(result.exitCode,0);
  const native=JSON.stringify(result.report);
  const formatted=await formatReport(result.report,{reporter:'json'},transport(root));
  const mte=JSON.parse(formatted);
  assert.equal(validate(mte),true,JSON.stringify(validate.errors));
  assert.equal(mte.schemaVersion,'2');assert.equal(mte.framework.name,'errspec');
  assert.equal(mte.files['src/value.ts'].source,readFileSync(join(root,'src/value.ts'),'utf8'));
  assert.deepEqual(mte.files['src/value.ts'].mutants.map(m=>m.id),result.report.mutants.map(m=>m.key));
  assert.ok(mte.files['src/value.ts'].mutants.some(m=>m.gap==='notInfected'));
  assert.ok(mte.files['src/value.ts'].mutants.some(m=>m.gap==='notReached'));
  const agent=JSON.parse(await formatReport(result.report,{reporter:'agent'},transport(root)));
  assert.ok(agent.gaps.some(g=>g.gap==='notInfected'&&g.coveredByCount===1&&g.scope.source.includes('function value')&&g.fix.includes('input')));
  assert.ok(agent.gaps.some(g=>g.gap==='notReached'&&g.coveredByCount===0));
  const simplify=await formatReport(result.report,{reporter:'simplify'},transport(root));
  assert.match(simplify,/statement/);assert.doesNotMatch(simplify,/unseen.*statement/);
  const html=await formatReport(result.report,{reporter:'html'},transport(root));
  assert.match(html,/<!doctype html>/);assert.match(html,/Original source/);assert.doesNotMatch(html,/<script[^>]+src=|<link[^>]+href=/);
  const payload=html.match(/<script type="application\/json" id="report-data">([\s\S]*?)<\/script>/)[1];
  assert.deepEqual(JSON.parse(payload),mte);
  // Verify the actual standalone viewer executes and filtering uses the embedded report.
  class Element {constructor(){this.children=[];this.value='';this.listeners={};}append(c){this.children.push(c);}replaceChildren(){this.children=[];}addEventListener(k,fn){this.listeners[k]=fn;}}
  const nodes=Object.fromEntries(['report-data','summary','warnings','status','query','count','files'].map(k=>[k,new Element()]));
  nodes['report-data'].textContent=payload;
  vm.runInNewContext(html.match(/<script>\n([\s\S]*?)<\/script>/)[1],{document:{getElementById:id=>nodes[id],createElement:()=>new Element()}});
  assert.match(nodes.count.textContent,/mutants shown/);assert.ok(nodes.files.children.length>0);
  nodes.query.value='does not exist';nodes.query.listeners.input();assert.equal(nodes.count.textContent,'0 mutants shown');
  assert.equal(JSON.stringify(result.report),native,'formatting must not alter judgments');
});
test('CLI saves selected files and keeps agent stdout one JSON document even in GitHub Actions',t=>{
  const root=project(t);
  const out=cli(root,['--reporter','agent,json,html','--report-dir','saved'],0,{GITHUB_ACTIONS:'true'});
  const agent=JSON.parse(out.stdout);assert.equal(agent.kind,'errspec/mutation-agent');
  assert.equal(validate(JSON.parse(readFileSync(join(root,'saved/mutation.json'),'utf8'))),true);
  assert.match(readFileSync(join(root,'saved/index.html'),'utf8'),/errspec mutation report/);
  assert.match(out.stderr,/report written to/);
  const native=cli(root,['--format','json']);assert.equal(JSON.parse(native.stdout).kind,'errspec/mutation');
  assert.doesNotMatch(native.stdout,/mutation-agent|"source":/);
  writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({reporters:['html'],reportDir:'configured'}));
  cli(root);assert.match(readFileSync(join(root,'configured/index.html'),'utf8'),/errspec/);
  const replaced=cli(root,['--reporter','agent']);assert.equal(JSON.parse(replaced.stdout).kind,'errspec/mutation-agent');
  cli(root,['--reporter','text,agent'],2);
});
test('formatter rejects stale spans and can use saved source snapshots with Unicode positions and inert scripts',async t=>{
  const root=project(t),source='export const title="🐈"; export const enabled = true;\n';
  writeFileSync(join(root,'src/value.ts'),source);
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value";test("enabled",()=>expect(enabled).toBeDefined());');
  const result=await runMutation({node:process.execPath},{binary,cwd:root});
  const native=result.report;
  writeFileSync(join(root,'src/value.ts'),'changed');
  await assert.rejects(formatReport(native,{reporter:'json'},transport(root)),/Source changed|offset/);
  rmSync(join(root,'src/value.ts'));
  const mte=JSON.parse(await formatReport(native,{reporter:'json',sources:{'src/value.ts':source}},transport(root)));
  assert.equal(validate(mte),true,JSON.stringify(validate.errors));
  const mutant=mte.files['src/value.ts'].mutants.find(m=>m.mutatorName==='BooleanLiteral');
  assert.deepEqual(mutant.location.start,{line:1,column:source.indexOf('true')+1});
  const dangerous='// </script><img src=x onerror=alert(1)>\n'+source;
  const shifted=structuredClone(native);
  for(const m of shifted.mutants){m.line++;m.byteOffset+=Buffer.byteLength(dangerous)-Buffer.byteLength(source);m.end+=Buffer.byteLength(dangerous)-Buffer.byteLength(source);}
  const html=await formatReport(shifted,{reporter:'html',sources:{'src/value.ts':dangerous}},transport(root));
  assert.doesNotMatch(html,/<img src=x/);assert.match(html,/\\u003c\/script>/);
  await assert.rejects(formatReport({...native,schemaVersion:9},{reporter:'json',sources:{'src/value.ts':source}},transport(root)),/Unsupported mutation report/);
  await assert.rejects(formatReport(native,{reporter:'wrong'},transport(root)),/unknown variant/);
});
test('annotations represent selected gaps, escape workflow syntax, preserve Pending and report quality failure',async t=>{
  const root=project(t),result=await runMutation({node:process.execPath,policy:{timeBudget:0,thresholdBreak:100}},{binary,cwd:root});
  assert.equal(result.exitCode,1);assert.equal(result.report.summary.score,null);
  const json=JSON.parse(await formatReport(result.report,{reporter:'json'},transport(root)));
  assert.ok(json.files['src/value.ts'].mutants.some(m=>m.status==='Pending'));
  assert.equal(json.errspec.summary.score,null);assert.equal(json.errspec.gate.passed,false);
  const survivor=structuredClone(result.report);survivor.mutants=survivor.mutants.slice(0,1);survivor.mutants[0].status='Survived';survivor.mutants[0].replacement='x\n::error::injected%';
  assert.equal(await formatReport(survivor,{reporter:'annotations'},transport(root)),'');
  survivor.selection={since:'HEAD',warnings:[]};
  const annotations=await formatReport(survivor,{reporter:'annotations'},transport(root));
  assert.match(annotations,/^::warning file=src\/value.ts,line=1,col=/);
  assert.match(annotations,/%0A::error::injected%25/);assert.equal(annotations.split('\n').length,1);
  cli(root,['--reporter','json','--time-budget','0','--threshold-break','100'],1);
  const gated=JSON.parse(readFileSync(join(root,'.errspec/report/mutation.json'),'utf8'));
  assert.equal(gated.errspec.gate.passed,false);assert.equal(gated.errspec.summary.score,null);
});
test('actual Git diff produces automatic annotations while native JSON remains a single document',t=>{
  const root=project(t);
  const git=args=>{const r=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(r.status,0,r.stderr);};
  git(['init','-q']);git(['add','src/value.ts','value.test.ts','vitest.config.mjs']);
  git(['-c','user.name=errspec','-c','user.email=errspec@example.test','commit','-qm','fixture']);
  writeFileSync(join(root,'src/value.ts'),'export function value(n:number){ console.log("observation",n); return n >= 0; }\nexport const unseen=()=>false;\n');
  const text=cli(root,['--since','HEAD'],0,{GITHUB_ACTIONS:'true'});
  assert.match(text.stdout,/::warning file=src\/value.ts,line=1,col=/);
  const native=cli(root,['--since','HEAD','--format','json'],0,{GITHUB_ACTIONS:'true'});
  assert.equal(JSON.parse(native.stdout).kind,'errspec/mutation');assert.doesNotMatch(native.stdout,/::warning/);
});
