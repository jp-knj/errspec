// Formatter-only tarball acceptance; does not execute mutation runners or inspect worker ownership.
import {mkdirSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {withInstalledCore} from './installed-core.mjs';
withInstalledCore(process.argv[2]??`artifacts/npm/manifest-${process.platform}-${process.arch}.json`,({root,manifest,run})=>{
  mkdirSync(join(root,'src'));
  const source='export function ready(){return true;}\n';
  writeFileSync(join(root,'src/value.ts'),source);
  const start=source.indexOf('true');
  const report={kind:'errspec/mutation',schemaVersion:1,engine:{tsrsRevision:'fixture',rulesVersion:'1',node:null,vitest:null,typecheck:'not_run',coverageScope:'vitest_vite_workers'},runComplete:true,ok:true,testsExecuted:true,baseline:[],summary:{total:1,killed:0,survived:1,noCoverage:0,timeout:0,runtimeError:0,ignored:0,pending:0,score:0},gate:{enabled:false,passed:null},diagnostics:[],mutants:[{key:'fixture-mutant',file:'src/value.ts',line:1,byteOffset:start,end:start+4,mutator:'BooleanLiteral',original:'true',replacement:'false',placement:'expression',ignored:null,regex:null,guardOffset:null,status:'Survived',coveredBy:['test-ready'],killedBy:[],failedModules:[],attempts:[]}]};
  writeFileSync(join(root,'report.json'),JSON.stringify(report));
  writeFileSync(join(root,'consumer.mjs'),`
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {formatReport} from '@errspec/core';
const report=JSON.parse(readFileSync('report.json','utf8'));
const schema=JSON.parse(readFileSync(new URL(import.meta.resolve('@errspec/core/report-schema')),'utf8'));
assert.equal(schema.title,'MutationTestResult');
const json=JSON.parse(await formatReport(report,{reporter:'json'}));
assert.equal(json.schemaVersion,'2');assert.equal(json.files['src/value.ts'].mutants[0].status,'Survived');
assert.match(json.files['src/value.ts'].source,/function ready/);
const agent=JSON.parse(await formatReport(report,{reporter:'agent'}));
assert.equal(agent.gaps[0].gap,'notDetected');assert.match(agent.gaps[0].scope.source,/function ready/);
assert.match(await formatReport(report,{reporter:'html'}),/errspec mutation report/);
assert.match(await formatReport(report,{reporter:'text'}),/Survived/);
assert.match(await formatReport(report,{reporter:'simplify'}),/No candidates/);
assert.equal(await formatReport(report,{reporter:'annotations'}),'');
await assert.rejects(formatReport({...report,schemaVersion:99},{reporter:'json'}),/Unsupported mutation report/);
`);
  run(process.execPath,['consumer.mjs']);
  console.log(JSON.stringify({target:manifest.target,checks:['installed-formatter-api','installed-upstream-schema','installed-agent-function-context','installed-offline-html','installed-text-simplify-annotations']},null,2));
});
