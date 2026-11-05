import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync,cpSync,existsSync} from 'node:fs';
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
function command(cwd,cmd,args,code=0,env={}) {
  const result=spawnSync(cmd,args,{cwd,env:{...process.env,...env},encoding:'utf8',timeout:60000});
  assert.ifError(result.error);assert.equal(result.status,code,result.stdout+result.stderr);return result;
}
function outputs(path) {
  const values={};const lines=readFileSync(path,'utf8').split('\n');
  for(let i=0;i<lines.length;i++){const found=/^([^<]+)<<(.+)$/.exec(lines[i]);if(!found)continue;const text=[];while(lines[++i]!==found[2])text.push(lines[i]);values[found[1]]=text.join('\n');}
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
