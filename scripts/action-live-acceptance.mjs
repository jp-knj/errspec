// Test-only coordinator for real uses:./ and actions/cache service acceptance.
// It never judges mutations or changes the Action's environment fingerprint rules.
import assert from 'node:assert/strict';
import {appendFileSync,existsSync,lstatSync,mkdirSync,mkdtempSync,readFileSync,readdirSync,realpathSync,rmSync,symlinkSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
import {prepare} from './action.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url));
function output(name,value){
  assert.doesNotMatch(String(value),/[\r\n]/);
  if(process.env.GITHUB_OUTPUT)appendFileSync(process.env.GITHUB_OUTPUT,`${name}=${value}\n`);
}
function git(root,args){
  const result=spawnSync('git',args,{cwd:root,encoding:'utf8'});
  assert.ifError(result.error);assert.equal(result.status,0,result.stderr);return result.stdout.trim();
}
export function cacheSnapshot(directory){
  const records=[];
  function visit(path,name=''){
    const info=lstatSync(path);assert.equal(info.isSymbolicLink(),false,'cache identity must not follow symlinks');
    if(info.isDirectory())for(const child of readdirSync(path).sort())visit(join(path,child),name?`${name}/${child}`:child);
    else{assert.ok(info.isFile(),'cache must contain regular files');const bytes=readFileSync(path);records.push({file:name,bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});}
  }
  visit(directory);assert.ok(records.length,'native cache must be nonempty');return records;
}
function state(){
  const path=realpathSync(process.env.ERRSPEC_LIVE_METADATA),value=JSON.parse(readFileSync(path,'utf8'));
  assert.equal(value.kind,'errspec/action-live-acceptance');assert.equal(dirname(path),value.control);
  assert.equal(realpathSync(value.project),join(value.control,'project'));
  assert.equal(value.cache,join(value.project,'.errspec/mutation-cache'));
  return {path,value};
}
function save(path,value){writeFileSync(path,JSON.stringify(value,null,2)+'\n',{mode:0o600});}
function report(directory,outcome,exitCode,status){
  assert.equal(process.env.ERRSPEC_LIVE_OUTCOME,outcome,'actual composite step outcome');
  assert.equal(process.env.ERRSPEC_LIVE_EXIT_CODE,String(exitCode),'actual composite exit-code output');
  assert.equal(process.env.ERRSPEC_LIVE_RUN_COMPLETE,'true','actual composite run-complete output');
  assert.equal(process.env.ERRSPEC_LIVE_REPORT_DIRECTORY,directory,'actual composite report-directory output');
  const native=JSON.parse(readFileSync(join(directory,'native.json'),'utf8'));
  assert.equal(native.kind,'errspec/mutation');assert.equal(native.runComplete,true);assert.equal(native.testsExecuted,true);
  assert.equal(native.ok,exitCode===0);assert.equal(native.gate.passed,exitCode===0);
  assert.equal(native.baseline.length,1);assert.ok(native.baseline.every(t=>t.started&&t.status==='passed'));
  assert.equal(native.mutants.length,1);assert.equal(native.mutants[0].status,status);
  assert.ok(native.cache.saved>0,'completed native evidence must be saved even on a quality failure');
  const mte=JSON.parse(readFileSync(join(directory,'mutation.json'),'utf8'));
  assert.equal(mte.schemaVersion,'2');assert.ok(Object.keys(mte.files).length);
  const agent=JSON.parse(readFileSync(join(directory,'agent.json'),'utf8'));
  assert.equal(agent.kind,'errspec/mutation-agent');assert.match(readFileSync(join(directory,'index.html'),'utf8'),/errspec mutation report/);
  return native;
}
export async function setup(){
  const control=realpathSync(mkdtempSync(join(process.env.RUNNER_TEMP||tmpdir(),'errspec-live-action-'))),project=join(control,'project');
  mkdirSync(join(project,'src'),{recursive:true});
  const binary=realpathSync(resolve(process.env.ERRSPEC_LIVE_NATIVE_BINARY||join(repo,'target/release/errspec')));
  const vitest=realpathSync(resolve(process.env.ERRSPEC_LIVE_VITEST_PACKAGE||join(repo,'examples/checkout/node_modules/vitest')));
  const version=JSON.parse(readFileSync(join(vitest,'package.json'),'utf8')).version;
  writeFileSync(join(project,'package.json'),JSON.stringify({private:true,type:'module',dependencies:{vitest:version}}));
  writeFileSync(join(project,'.gitignore'),'node_modules/\n.errspec/\n');
  writeFileSync(join(project,'src/value.ts'),'export const enabled=true;\n');
  writeFileSync(join(project,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value.ts";test("enabled",()=>expect(enabled).toBeDefined());\n');
  writeFileSync(join(project,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};\n');
  git(project,['init','-q']);git(project,['add','.']);
  git(project,['-c','user.name=errspec','-c','user.email=errspec@example.test','-c','commit.gpgSign=false','commit','-qm','Action acceptance baseline']);
  const base=git(project,['rev-parse','HEAD']);
  writeFileSync(join(project,'src/value.ts'),'export const enabled=false;\n');
  mkdirSync(join(project,'node_modules'));symlinkSync(vitest,join(project,'node_modules/vitest'),'dir');
  const firstReport=join(control,'first-report'),secondReport=join(control,'second-report');
  const options=JSON.stringify({vitestPackage:vitest,reuseSession:true,policy:{failOnSurvived:true}});
  // Reuse the real prepare path for the exact private key; no new Action output/API.
  Object.assign(process.env,{ERRSPEC_WORKING_DIRECTORY:project,ERRSPEC_NATIVE_BINARY:binary,ERRSPEC_SINCE:base,ERRSPEC_OPTIONS:options,ERRSPEC_CONFIG_FILE:'',ERRSPEC_CACHE:'true',ERRSPEC_CACHE_DIRECTORY:'.errspec/mutation-cache',ERRSPEC_REPORT_DIRECTORY:firstReport});
  const prepared=await prepare(),metadata=join(control,'acceptance.json'),evidence=join(control,'evidence.json');
  save(metadata,{kind:'errspec/action-live-acceptance',phase:'prepared',control,project,binary,base,options,cache:prepared.cacheDirectory,key:prepared.key,preparedState:prepared.state,firstReport,secondReport,evidence});
  for(const [name,value]of Object.entries({metadata,project,binary,base,options,'cache-path':prepared.cacheDirectory,'cache-key':prepared.key,'first-report':firstReport,'second-report':secondReport,evidence}))output(name,value);
  return metadata;
}
export function verifyFailure(){
  const {path,value}=state();assert.equal(value.phase,'prepared');
  const first=report(value.firstReport,'failure',1,'Survived');
  value.keys=first.mutants.map(m=>m.key);value.cacheIdentity=cacheSnapshot(value.cache);
  // The subsequent service restore must recreate bytes, not find a local leftover.
  rmSync(value.cache,{recursive:true});assert.equal(existsSync(value.cache),false);
  value.phase='cache-cleared';save(path,value);
}
export function verifyRestored(){
  const {path,value}=state();assert.equal(value.phase,'cache-cleared');
  assert.equal(process.env.ERRSPEC_LIVE_CACHE_HIT,'true','GitHub must report an exact service cache hit');
  assert.equal(process.env.ERRSPEC_LIVE_MATCHED_KEY,value.key,'restore the key saved by the failed composite');
  assert.deepEqual(cacheSnapshot(value.cache),value.cacheIdentity,'service restore must reproduce the native cache content');
  // A changed current test must invalidate old Survived evidence, whatever GitHub's
  // current environment is. Do not normalize GITHUB_* or demand cross-step reuse.
  writeFileSync(join(value.project,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value.ts";test("enabled",()=>expect(enabled).toBe(false));\n');
  value.phase='restored';save(path,value);
}
export function verifyRerun(){
  const {path,value}=state();assert.equal(value.phase,'restored');
  const current=report(value.secondReport,'success',0,'Killed');
  assert.deepEqual(current.mutants.map(m=>m.key),value.keys,'same source edit after test improvement');
  assert.equal(current.cache.reused,0,'changed test must not reuse the old survivor');assert.ok(current.cache.executed>0);
  assert.equal(readFileSync(join(value.project,'src/value.ts'),'utf8'),'export const enabled=false;\n');
  save(value.evidence,{kind:'errspec/action-live-evidence',savedAfterQualityFailure:true,localCacheRemoved:true,exactServiceCacheHit:true,restoredContent:value.cacheIdentity,currentBaselinePassed:true,sameMutantRevalidated:true,oldSurvivorReused:false});
  value.phase='verified';save(path,value);
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  try{const operation=process.argv[2];if(operation==='setup')await setup();else if(operation==='verify-failure')verifyFailure();else if(operation==='verify-restored')verifyRestored();else if(operation==='verify-rerun')verifyRerun();else throw Error('Unknown live Action acceptance operation');}
  catch(error){process.stderr.write(`${error.stack||error}\n`);process.exitCode=1;}
}
