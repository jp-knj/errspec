// Formatter-only tarball acceptance; does not execute mutation runners or inspect worker ownership.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {targets} from '../packages/cli/platforms.mjs';
const manifestPath=resolve(process.argv[2]??`artifacts/npm/manifest-${process.platform}-${process.arch}.json`);
const manifest=JSON.parse(readFileSync(manifestPath,'utf8'));
assert.equal(manifest.target,`${process.platform}-${process.arch}`,'Test the current platform tarball');
const repo=fileURLToPath(new URL('../',import.meta.url));
const root=mkdtempSync(join(tmpdir(),'errspec-report-consumer-'));
function run(command,args){const r=spawnSync(command,args,{cwd:root,encoding:'utf8',timeout:60000});assert.equal(r.status,0,r.stdout+r.stderr);return r.stdout;}
try {
  const dependencies={},packages={};
  for(const pkg of manifest.packages.filter(pkg=>['@errspec/core','@errspec/cli',`@errspec/cli-${manifest.target}`].includes(pkg.name))) {
    const path=resolve(manifestPath,'..',pkg.filename),bytes=readFileSync(path);
    assert.equal(createHash('sha256').update(bytes).digest('hex'),pkg.sha256);
    dependencies[pkg.name]=`file:${path}`;
    const metadata=pkg.name.startsWith('@errspec/cli-')?{os:[process.platform],cpu:[process.arch]}:JSON.parse(readFileSync(join(repo,'packages',pkg.name.split('/')[1],'package.json'),'utf8'));
    packages[`node_modules/${pkg.name}`]={version:pkg.version,resolved:`file:${path}`,integrity:`sha512-${createHash('sha512').update(bytes).digest('base64')}`,license:'MIT',...Object.fromEntries(['dependencies','optionalDependencies','engines','bin','os','cpu'].filter(k=>metadata[k]).map(k=>[k,metadata[k]]))};
  }
  // npm ci validates every optional dependency even when its OS/CPU excludes it.
  for(const [target,info] of Object.entries(targets)) {
    const name=`@errspec/cli-${target}`;
    packages[`node_modules/${name}`]??={version:manifest.version,optional:true,os:[info.os],cpu:[info.cpu],resolved:`file:unavailable-${target}.tgz`};
  }
  const project={name:'errspec-report-consumer',private:true,type:'module',dependencies};
  packages['']=project;
  writeFileSync(join(root,'package.json'),JSON.stringify(project));
  writeFileSync(join(root,'package-lock.json'),JSON.stringify({name:project.name,lockfileVersion:3,requires:true,packages}));
  run('npm',['ci','--offline','--ignore-scripts','--no-audit','--no-fund','--cache',join(root,'cache')]);
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
} finally {rmSync(root,{recursive:true,force:true});}
