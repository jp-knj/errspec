import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync,cpSync,existsSync,readdirSync,lstatSync,appendFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
const require=createRequire(new URL('../packages/report-tests/package.json',import.meta.url));
const {parse}=require('yaml');
const repo=fileURLToPath(new URL('../',import.meta.url));
const binary=process.env.ERRSPEC_TEST_BINARY||join(repo,'target/debug/errspec');
const action=join(repo,'scripts/action.mjs'),metadata=parse(readFileSync(join(repo,'action.yml'),'utf8'));
const live=join(repo,'scripts/action-live-acceptance.mjs');
function command(cwd,cmd,args,code=0,env={}) {
  const result=spawnSync(cmd,args,{cwd,env:{...process.env,...env},encoding:'utf8',timeout:60000});
  assert.ifError(result.error);assert.equal(result.status,code,result.stdout+result.stderr);return result;
}
function outputs(path) {
  const values={};const lines=readFileSync(path,'utf8').split('\n');
  for(let i=0;i<lines.length;i++){const found=/^([^<]+)<<(.+)$/.exec(lines[i]);if(!found){const single=/^([^=]+)=(.*)$/.exec(lines[i]);if(single)values[single[1]]=single[2];continue;}const text=[];while(lines[++i]!==found[2])text.push(lines[i]);values[found[1]]=text.join('\n');}
  return values;
}
function fixture(t) {
  const control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-action-')));t.after(()=>rmSync(control,{recursive:true,force:true}));
  const origin=join(control,'origin.git'),source=join(control,'source'),project=join(control,'project');
  command(control,'git',['init','--bare','-q',origin]);command(control,'git',['clone','-q',origin,source]);
  mkdirSync(join(source,'src'));
  writeFileSync(join(source,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
  writeFileSync(join(source,'src/value.ts'),'export const enabled=true;\n');
  writeFileSync(join(source,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value";test("enabled",()=>expect(enabled).toBeDefined());');
  writeFileSync(join(source,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');
  command(source,'git',['add','.']);command(source,'git',['-c','user.name=errspec','-c','user.email=errspec@example.test','commit','-qm','base']);
  const base=command(source,'git',['rev-parse','HEAD']).stdout.trim();
  writeFileSync(join(source,'src/value.ts'),'export const enabled=false;\n');
  command(source,'git',['add','src/value.ts']);command(source,'git',['-c','user.name=errspec','-c','user.email=errspec@example.test','commit','-qm','change']);
  command(source,'git',['push','-q','origin','HEAD']);
  function checkout(path){command(control,'git',['clone','-q','--depth=1',pathToFileURL(origin).href,path]);mkdirSync(join(path,'node_modules'));symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(path,'node_modules/vitest'));}
  checkout(project);
  const output=join(control,'output'),summary=join(control,'summary'),event=join(control,'event.json');
  writeFileSync(output,'');writeFileSync(summary,'');writeFileSync(event,JSON.stringify({pull_request:{base:{sha:base,ref:'main'}}}));
  const env={GITHUB_WORKSPACE:project,GITHUB_EVENT_PATH:event,GITHUB_OUTPUT:output,GITHUB_STEP_SUMMARY:summary,GITHUB_REF:'refs/pull/1/merge',GITHUB_SHA:command(project,'git',['rev-parse','HEAD']).stdout.trim(),GITHUB_RUN_ID:'fixture',GITHUB_RUN_ATTEMPT:'1',RUNNER_TEMP:control,RUNNER_OS:'fixture'};
  return {control,origin,source,project,base,output,summary,env,checkout};
}
test('composite action wires official restore/save with an always save after a failed run',()=>{
  assert.equal(metadata.runs.using,'composite');
  const restore=metadata.runs.steps.find(s=>s.uses==='actions/cache/restore@v4'),save=metadata.runs.steps.find(s=>s.uses==='actions/cache/save@v4');
  assert.ok(restore);assert.ok(save);
  assert.match(save.if,/always\(\)/);assert.match(save.if,/steps\.prepare\.outcome == 'success'/);
  assert.equal(save.with.path,restore.with.path);assert.equal(save.with.key,restore.with.key);
  assert.ok(metadata.runs.steps.indexOf(save)>metadata.runs.steps.findIndex(s=>s.id==='run'));
  for(const step of metadata.runs.steps.filter(s=>s.run))assert.doesNotMatch(step.run,/\$\{\{.*inputs\./,'inputs must enter through environment, never shell source');
});
test('Distribution invokes the actual composite and proves save-on-failure through an exact service restore on every platform',()=>{
  const workflow=parse(readFileSync(join(repo,'.github/workflows/distribution.yml'),'utf8')),job=workflow.jobs.packages;
  assert.deepEqual(job.strategy.matrix.include.map(m=>m.target),['linux-x64','darwin-arm64','darwin-x64']);
  const steps=job.steps,fixture=steps.find(s=>s.id==='live_fixture'),first=steps.find(s=>s.id==='live_failure'),restore=steps.find(s=>s.id==='live_service_restore'),second=steps.find(s=>s.id==='live_rerun');
  assert.match(fixture.run,/action-live-acceptance\.mjs setup/);
  assert.equal(first.uses,'./');assert.equal(first['continue-on-error'],true);assert.equal(second.uses,'./');assert.notEqual(second['continue-on-error'],true);
  assert.equal(first.with.options,'${{ steps.live_fixture.outputs.options }}');assert.equal(second.with.options,first.with.options);
  assert.equal(first.with.cache,'true');assert.equal(second.with.cache,'true');
  assert.equal(restore.uses,'actions/cache/restore@v4');assert.equal(restore.with['fail-on-cache-miss'],true);assert.equal(restore['continue-on-error'],undefined);
  assert.equal(restore.with.key,'${{ steps.live_fixture.outputs.cache-key }}');assert.equal(restore.with.path,'${{ steps.live_fixture.outputs.cache-path }}');assert.equal(restore.with['restore-keys'],undefined);
  const clear=steps.find(s=>s.run?.includes('action-live-acceptance.mjs verify-failure')),restored=steps.find(s=>s.run?.includes('action-live-acceptance.mjs verify-restored')),verified=steps.find(s=>s.run?.includes('action-live-acceptance.mjs verify-rerun'));
  const order=[fixture,first,clear,restore,restored,second,verified].map(s=>steps.indexOf(s));assert.ok(order.every((n,i)=>n>=0&&(!i||n>order[i-1])));
  for(const [step,id]of [[clear,'live_failure'],[verified,'live_rerun']]){
    assert.equal(step.env.ERRSPEC_LIVE_OUTCOME,'${{ steps.'+id+'.outcome }}');
    for(const [name,key]of [['EXIT_CODE','exit-code'],['RUN_COMPLETE','run-complete'],['REPORT_DIRECTORY','report-directory']])assert.equal(step.env['ERRSPEC_LIVE_'+name],'${{ steps.'+id+'.outputs.'+key+' }}');
  }
  assert.equal(restored.env.ERRSPEC_LIVE_CACHE_HIT,'${{ steps.live_service_restore.outputs.cache-hit }}');assert.equal(restored.env.ERRSPEC_LIVE_MATCHED_KEY,'${{ steps.live_service_restore.outputs.cache-matched-key }}');
  const artifact=steps.find(s=>s.name==='Retain live Action acceptance evidence');assert.match(artifact.if,/always\(\)/);assert.match(artifact.with.path,/first-report/);assert.match(artifact.with.path,/second-report/);assert.match(artifact.with.path,/outputs\.evidence/);
  for(const step of [fixture,first,clear,restore,restored,second,verified])assert.ok(!Object.keys(step.env||{}).some(key=>key.startsWith('GITHUB_')),'live acceptance must keep the real GitHub environment');
});
test('live acceptance helpers verify real failed reports, removed cache identity and current native revalidation',t=>{
  const control=realpathSync(mkdtempSync(join(tmpdir(),'errspec-live-action-test-')));t.after(()=>rmSync(control,{recursive:true,force:true}));
  const output=join(control,'output'),summary=join(control,'summary');writeFileSync(output,'');writeFileSync(summary,'');
  const env={RUNNER_TEMP:control,GITHUB_WORKSPACE:repo,GITHUB_EVENT_PATH:'',GITHUB_OUTPUT:output,GITHUB_STEP_SUMMARY:summary,GITHUB_REF:'refs/heads/local-live-fixture',GITHUB_SHA:'fixture',GITHUB_RUN_ID:'fixture',GITHUB_RUN_ATTEMPT:'1',RUNNER_OS:'fixture',ERRSPEC_LIVE_NATIVE_BINARY:binary,ERRSPEC_LIVE_VITEST_PACKAGE:join(repo,'examples/checkout/node_modules/vitest')};
  command(repo,process.execPath,[live,'setup'],0,env);
  const prepared=outputs(output),data=JSON.parse(readFileSync(prepared.metadata,'utf8'));
  command(repo,process.execPath,[action,'run'],1,{...env,ERRSPEC_ACTION_STATE:data.preparedState});
  const first=outputs(output);assert.match(readFileSync(summary,'utf8'),/quality gate failed/);
  const saved=join(control,'emulated-service-copy');cpSync(data.cache,saved,{recursive:true});
  const verify={...env,ERRSPEC_LIVE_METADATA:prepared.metadata,ERRSPEC_LIVE_OUTCOME:'failure',ERRSPEC_LIVE_EXIT_CODE:first['exit-code'],ERRSPEC_LIVE_RUN_COMPLETE:first['run-complete'],ERRSPEC_LIVE_REPORT_DIRECTORY:first['report-directory']};
  command(repo,process.execPath,[live,'verify-failure'],1,{...verify,ERRSPEC_LIVE_EXIT_CODE:'0'});
  command(repo,process.execPath,[live,'verify-failure'],0,verify);assert.equal(existsSync(data.cache),false);
  // Local directory transfer tests the verifier, not GitHub's service. Only the
  // Distribution uses:./ + actions/cache/restore steps provide live proof.
  cpSync(saved,data.cache,{recursive:true});
  const restored={...env,ERRSPEC_LIVE_METADATA:prepared.metadata,ERRSPEC_LIVE_CACHE_HIT:'true',ERRSPEC_LIVE_MATCHED_KEY:data.key};
  command(repo,process.execPath,[live,'verify-restored'],1,{...restored,ERRSPEC_LIVE_CACHE_HIT:'false'});
  command(repo,process.execPath,[live,'verify-restored'],1,{...restored,ERRSPEC_LIVE_MATCHED_KEY:'different-key'});
  const cached=readdirSync(data.cache).find(name=>lstatSync(join(data.cache,name)).isFile());assert.ok(cached);
  appendFileSync(join(data.cache,cached),'tampered');command(repo,process.execPath,[live,'verify-restored'],1,restored);
  rmSync(data.cache,{recursive:true});cpSync(saved,data.cache,{recursive:true});command(repo,process.execPath,[live,'verify-restored'],0,restored);
  writeFileSync(output,'');
  command(repo,process.execPath,[action,'prepare'],0,{...env,ERRSPEC_WORKING_DIRECTORY:data.project,ERRSPEC_NATIVE_BINARY:data.binary,ERRSPEC_SINCE:data.base,ERRSPEC_OPTIONS:data.options,ERRSPEC_REPORT_DIRECTORY:data.secondReport});
  const rerun=outputs(output);command(repo,process.execPath,[action,'run'],0,{...env,GITHUB_ACTION:'current-rerun',ERRSPEC_ACTION_STATE:rerun.state});
  const final=outputs(output);command(repo,process.execPath,[live,'verify-rerun'],0,{...env,ERRSPEC_LIVE_METADATA:prepared.metadata,ERRSPEC_LIVE_OUTCOME:'success',ERRSPEC_LIVE_EXIT_CODE:final['exit-code'],ERRSPEC_LIVE_RUN_COMPLETE:final['run-complete'],ERRSPEC_LIVE_REPORT_DIRECTORY:final['report-directory']});
  const evidence=JSON.parse(readFileSync(data.evidence,'utf8'));assert.equal(evidence.sameMutantRevalidated,true);assert.equal(evidence.oldSurvivorReused,false);assert.ok(evidence.restoredContent.length);
});
test('actual shallow Git base fetch, native failure reports, annotations and carried history share native verdicts',t=>{
  const f=fixture(t);
  assert.notEqual(spawnSync('git',['cat-file','-e',`${f.base}^{commit}`],{cwd:f.project}).status,0);
  const prepare=(root=f.project)=>{
    writeFileSync(f.output,'');
    command(f.project,process.execPath,[action,'prepare'],0,{...f.env,ERRSPEC_WORKING_DIRECTORY:root,ERRSPEC_NATIVE_BINARY:binary,ERRSPEC_OPTIONS:JSON.stringify({cacheRelocation:true,policy:{failOnSurvived:true}})});
    return outputs(f.output);
  };
  const first=prepare();assert.ok(first['cache-key'].includes('fixture-1'));
  assert.equal(command(f.project,'git',['rev-parse',`${f.base}^{commit}`]).stdout.trim(),f.base);
  const run=command(f.project,process.execPath,[action,'run'],1,{...f.env,ERRSPEC_ACTION_STATE:first.state});
  assert.match(run.stdout,/::warning file=src\/value.ts/);
  const native=JSON.parse(readFileSync(join(first['report-directory'],'native.json'),'utf8'));
  assert.equal(native.runComplete,true);assert.equal(native.ok,false);assert.ok(native.summary.survived>0);
  assert.ok(existsSync(join(first['report-directory'],'index.html')));assert.match(readFileSync(f.summary,'utf8'),/quality gate failed/);
  assert.equal(outputs(f.output)['exit-code'],'1');
  // The service's restore/save steps are external; emulate their directory transfer after
  // the failed run, while executing the real preparation and native mutation scripts.
  const saved=join(f.control,'saved-cache');cpSync(first['cache-path'],saved,{recursive:true});
  const secondProject=join(f.control,'second');f.checkout(secondProject);
  const second=prepare(secondProject);mkdirSync(join(secondProject,'.errspec'),{recursive:true});cpSync(saved,second['cache-path'],{recursive:true});
  command(f.project,process.execPath,[action,'run'],1,{...f.env,ERRSPEC_ACTION_STATE:second.state});
  const repeated=JSON.parse(readFileSync(join(second['report-directory'],'native.json'),'utf8'));
  assert.ok(repeated.cache.reused>0,JSON.stringify(repeated.cache));assert.equal(repeated.gate.passed,false);
});
test('invalid JSON, unavailable engines and dangerous Git option-shaped refs stop during preparation',t=>{
  const f=fixture(t);
  for(const extra of [{ERRSPEC_OPTIONS:'[]'},{ERRSPEC_OPTIONS:'{"root":"elsewhere"}'},{ERRSPEC_SINCE:'--upload-pack=anything'},{ERRSPEC_NATIVE_BINARY:'missing-native'},{ERRSPEC_NATIVE_BINARY:''}]) {
    writeFileSync(f.output,'');
    command(f.project,process.execPath,[action,'prepare'],2,{...f.env,ERRSPEC_NATIVE_BINARY:binary,...extra});
    assert.equal(readFileSync(f.output,'utf8'),'');
  }
});

test('baseline failures retain restored history and write incomplete reports with exit two',t=>{
  const f=fixture(t),cache=join(f.project,'.errspec/mutation-cache');
  mkdirSync(cache,{recursive:true});writeFileSync(join(cache,'previous-history'),'retained');
  writeFileSync(join(f.project,'value.test.ts'),'import {test} from "vitest";test("baseline",()=>{throw Error("baseline fixture failure")});');
  writeFileSync(join(f.project,'selected-config.json'),JSON.stringify({policy:{thresholdHigh:93}}));
  command(f.project,process.execPath,[action,'prepare'],0,{...f.env,ERRSPEC_NATIVE_BINARY:binary,ERRSPEC_CONFIG_FILE:'selected-config.json'});
  const setup=outputs(f.output);
  command(f.project,process.execPath,[action,'run'],2,{...f.env,ERRSPEC_ACTION_STATE:setup.state});
  const native=JSON.parse(readFileSync(join(setup['report-directory'],'native.json'),'utf8'));
  assert.equal(native.runComplete,false);assert.equal(native.summary.score,null);
  assert.equal(native.policy.options.thresholdHigh,93);
  assert.match(readFileSync(f.summary,'utf8'),/incomplete/);
  assert.equal(readFileSync(join(cache,'previous-history'),'utf8'),'retained');
  assert.equal(outputs(f.output)['exit-code'],'2');
});
