// GitHub composite-action transport. Native runMutation owns every verdict and cache decision.
import {appendFileSync,readFileSync,writeFileSync,mkdirSync,mkdtempSync,realpathSync,statSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash,randomBytes} from 'node:crypto';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
import {runMutation,formatReport} from '../packages/core/index.mjs';
const hash=value=>createHash('sha256').update(value).digest('hex').slice(0,16);
function output(name,value) {
  if(!process.env.GITHUB_OUTPUT)return;
  const delimiter=`errspec_${randomBytes(16).toString('hex')}`;
  appendFileSync(process.env.GITHUB_OUTPUT,`${name}<<${delimiter}\n${value}\n${delimiter}\n`);
}
function git(root,args) {
  const result=spawnSync('git',args,{cwd:root,encoding:'utf8',timeout:60000,stdio:['ignore','pipe','pipe']});
  if(result.status!==0)throw new Error(`git ${args[0]} failed: ${result.stderr||result.error?.message||result.signal}`);
  return result.stdout.trim();
}
function input(name,fallback=''){return process.env[`ERRSPEC_${name}`]??fallback;}
function boolean(name,defaultValue){const value=input(name,defaultValue?'true':'false');if(!['true','false'].includes(value))throw new Error(`${name} must be true or false`);return value==='true';}
export async function prepare() {
  if(Number(process.versions.node.split('.')[0])<24)throw new Error('The errspec Action requires Node.js 24 or later');
  const workspace=resolve(process.env.GITHUB_WORKSPACE||process.cwd());
  const root=realpathSync(resolve(workspace,input('WORKING_DIRECTORY','.')));
  if(!statSync(root).isDirectory())throw new Error('working-directory must be a project directory');
  const options=JSON.parse(input('OPTIONS','{}'));
  if(!options||Array.isArray(options)||typeof options!=='object')throw new Error('options must be a JSON object');
  if('root' in options||'since' in options)throw new Error('Use working-directory and since inputs instead of options.root/options.since');
  const event=process.env.GITHUB_EVENT_PATH?JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH,'utf8')):{};
  const since=input('SINCE')||event.pull_request?.base?.sha||null;
  if(since!==null) {
    if(typeof since!=='string'||since.startsWith('-')||/[\r\n\0]/.test(since))throw new Error('since must be a Git ref or commit');
    const known=spawnSync('git',['rev-parse','--verify','--end-of-options',`${since}^{commit}`],{cwd:root,encoding:'utf8'});
    if(known.status!==0)git(root,['fetch','--no-tags','--depth=1','--','origin',since]);
    git(root,['rev-parse','--verify','--end-of-options',`${since}^{commit}`]);
  }
  let binary=input('NATIVE_BINARY');
  if(binary)binary=realpathSync(resolve(root,binary));
  else {
    const require=createRequire(join(root,'package.json'));
    const module=await import(pathToFileURL(require.resolve('@errspec/cli/native')).href);
    binary=module.nativeBinary();
  }
  const cache=boolean('CACHE',true),cacheDirectory=resolve(root,input('CACHE_DIRECTORY','.errspec/mutation-cache'));
  const reportDirectory=resolve(root,input('REPORT_DIRECTORY','.errspec/action-report'));
  const prefix=`errspec-mutation-v1-${process.env.RUNNER_OS||process.platform}-${hash(input('WORKING_DIRECTORY','.'))}`;
  const ref=process.env.GITHUB_REF||'local',base=event.pull_request?.base?.ref?`refs/heads/${event.pull_request.base.ref}`:ref;
  const refPrefix=`${prefix}-${hash(ref)}-`,basePrefix=`${prefix}-${hash(base)}-`;
  const key=`${refPrefix}${process.env.GITHUB_SHA||'local'}-${process.env.GITHUB_RUN_ID||'local'}-${process.env.GITHUB_RUN_ATTEMPT||'1'}`;
  const directory=mkdtempSync(join(process.env.RUNNER_TEMP||tmpdir(),'errspec-action-'));
  const state=join(directory,'state.json');
  writeFileSync(state,JSON.stringify({root,binary,reportDirectory,configFile:input('CONFIG_FILE')||undefined,options:{...options,root,since,cache,cacheDir:cacheDirectory,node:options.node??process.execPath}}),{mode:0o600});
  for(const [name,value] of Object.entries({state,'cache-path':cacheDirectory,'cache-key':key,'restore-ref':refPrefix,'restore-base':basePrefix,'report-directory':reportDirectory}))output(name,value);
  return {state,cacheDirectory,reportDirectory,key,refPrefix,basePrefix};
}
export async function run(statePath) {
  const state=JSON.parse(readFileSync(statePath,'utf8'));
  delete process.env.ERRSPEC_ACTION_STATE;
  const controller=new AbortController();
  const interrupt=()=>controller.abort();
  process.once('SIGINT',interrupt);process.once('SIGTERM',interrupt);
  let result;
  try {result=await runMutation(state.options,{binary:state.binary,cwd:state.root,configFile:state.configFile,signal:controller.signal});}
  finally {process.removeListener('SIGINT',interrupt);process.removeListener('SIGTERM',interrupt);}
  mkdirSync(state.reportDirectory,{recursive:true});
  writeFileSync(join(state.reportDirectory,'native.json'),JSON.stringify(result.report,null,2)+'\n');
  for(const [reporter,name] of [['json','mutation.json'],['html','index.html'],['agent','agent.json']]) {
    writeFileSync(join(state.reportDirectory,name),await formatReport(result.report,{reporter,root:state.root},{binary:state.binary,cwd:state.root}));
  }
  const annotations=await formatReport(result.report,{reporter:'annotations',root:state.root},{binary:state.binary,cwd:state.root});
  if(annotations)process.stdout.write(annotations+'\n');
  const s=result.report.summary,score=s.score===null?'unknown':`${(s.score*100).toFixed(2)}%`;
  const summary=`### errspec mutation testing\n\nResult: **${result.report.runComplete?(result.exitCode===0?'passed':'quality gate failed'):'incomplete'}** (exit ${result.exitCode}).\n\n| Total | Killed | Survived | Not reached | Pending | Score |\n| --- | --- | --- | --- | --- | --- |\n| ${s.total} | ${s.killed} | ${s.survived} | ${s.noCoverage} | ${s.pending} | ${score} |\n\nNative JSON, agent JSON, MTE JSON and offline HTML are saved in the report directory. Cached results remain subject to native fingerprint validation.\n`;
  if(process.env.GITHUB_STEP_SUMMARY)appendFileSync(process.env.GITHUB_STEP_SUMMARY,summary);
  output('exit-code',String(result.exitCode));output('run-complete',String(result.report.runComplete));
  return result.exitCode;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if(process.argv[2]==='prepare')await prepare();
    else if(process.argv[2]==='run')process.exitCode=await run(input('ACTION_STATE'));
    else throw new Error('Use prepare or run');
  }catch(error){
    process.stderr.write(`errspec Action: ${error.message}\n`);
    if(process.env.GITHUB_STEP_SUMMARY)appendFileSync(process.env.GITHUB_STEP_SUMMARY,'### errspec mutation testing\n\nExecution failed before a complete Action report was written. See the step log for the cause.\n');
    process.exitCode=2;
  }
}
