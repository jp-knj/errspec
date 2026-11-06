import { readFileSync, writeFileSync, realpathSync, existsSync, globSync, statSync } from "node:fs";
import { basename, dirname, resolve, relative, join, isAbsolute, matchesGlob } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { format } from "node:util";
import { serialize } from "node:v8";
import { instrument } from "./emitter.mjs";
import { typecheckMutants } from "./typecheck.mjs";
import { compilerInputs } from "./cache-inputs.mjs";
import { loadPlugins } from "./plugins.mjs";
import { registerSources, generatePluginSources, rememberProjectManifests } from "./source-inventory.mjs";
import { detectRunner, resolvePackage } from "./runners.mjs";
import { createJestAdapter } from "./jest.mjs";
import { createPlaywrightAdapter } from "./playwright.mjs";

const protocolWrite = process.stdout.write.bind(process.stdout);
// Config evaluation happens in this process, before Vitest redirects test logs.
process.stdout.write = (chunk, encoding, cb) => process.stderr.write(chunk, encoding, cb);
console.log = (...args) => process.stderr.write(format(...args)+"\n");
console.info = console.log;
const PROTOCOL = 1;
let options, root, runId, api, packageDir, toolTemp, inventory = new Map(), fingerprints = new Map();
let plain, baseline, testSpecs, projectName, configFiles = new Set();
let current, adapter, runner;
let infrastructureFailure;
let warmContext, reuseMode="fresh_lifecycle_unknown";
let configurationHash=null;
const pendingGenerated = new Map();
let mutationPlugins;
const normalize = s => s.replaceAll("\\", "/");
const rel = file => normalize(relative(root,file));
const hash = data => createHash("sha256").update(data).digest("hex");
const relocatedPath = path => {
  if(!options.cacheRelocation||!isAbsolute(path))return path;
  const name=rel(path);
  return name!=='..'&&!name.startsWith('../')&&!isAbsolute(name) ? {projectRootRelativePath:name} : path;
};
function failure(code,message) { const e = new Error(message); e.code=code; return e; }
function remember(file,content) {
  if (!existsSync(file) || !statSync(file).isFile()) return;
  file=realpathSync(file);
  const next=hash(readFileSync(file));
  if(content!==undefined&&hash(content)!==next)
    throw failure("EC_MUTATION_INPUT",`Input changed while being acquired: ${file}`);
  if (fingerprints.has(file) && fingerprints.get(file)!==next)
    throw failure("EC_MUTATION_INPUT",`Input changed during mutation run: ${file}`);
  fingerprints.set(file,next);
}
function verifyInputs() {
  for (const [file,digest] of fingerprints) {
    if (!existsSync(file) || hash(readFileSync(file))!==digest)
      throw failure("EC_MUTATION_INPUT",`Input changed during mutation run: ${file}`);
  }
}
function native(source,file,validateOnly=false,pluginInputs={},facts=false) {
  const result=spawnSync(options.binary,["__mutation-source"], {
    input:JSON.stringify({source,file,validateOnly,excludeMutators:options.excludeMutator,weakMutation:options.weakMutation,arid:options.arid,aridCallees:options.aridCallees,facts:!!(facts || options.since || options.cache && options.cacheScope || options.plugins?.length),customMutators:mutationPlugins?.names,pluginAridCallees:mutationPlugins?.aridCallees,...pluginInputs,engineFingerprint:options.engineFingerprint}),
    encoding:"utf8",maxBuffer:64*1024*1024,timeout:60000,
  });
  if(result.status!==0) throw failure(result.stderr?.startsWith("EC_MUTATION_INPUT:")?"EC_MUTATION_INPUT":"EC_MUTATION_INSTRUMENTATION",result.stderr||"Native parser failed");
  try {return JSON.parse(result.stdout);} catch {throw failure("EC_MUTATION_PROTOCOL","Invalid native generator response");}
}
async function generateSource(source,file,base) {
  if(!options.plugins?.length)return base??native(source,file);
  const acquired=base??native(source,file);
  const edits=await mutationPlugins.edits(source,file,acquired.metadata);
  return native(source,file,false,edits);
}
function candidateFiles(excluded) {
  const patterns=options.include.length?options.include:["src/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}"];
  const files=new Set();
  for(const pattern of patterns)for(const entry of globSync(pattern,{cwd:root,exclude:["**/node_modules/**",...options.exclude]})) {
    const absolute=realpathSync(resolve(root,entry)); const name=rel(absolute);
    if(absolute===toolTemp||absolute.startsWith(toolTemp+"/")||name.startsWith("../")||isAbsolute(name)||name.split("/").includes("node_modules")
      || !/\.(?:[cm]?[jt]s|[jt]sx)$/.test(name)||/\.d\.(?:[cm]?ts)$/.test(name)
      ||excluded.has(absolute)||!statSync(absolute).isFile())continue;
    files.add(absolute);
  }
  return [...files].sort((a,b)=>rel(a)<rel(b)?-1:rel(a)>rel(b)?1:0);
}
function specIdentities(specs) {
  return specs.map(s=>({file:s.moduleId,project:s.project.config.name||""}))
    .sort((a,b)=>a.file<b.file?-1:a.file>b.file?1:a.project<b.project?-1:a.project>b.project?1:0);
}
function plugin(phase) {
  return {name:"errspec-mutation",enforce:"pre",configResolved(config) {
    if(config.configFile)configFiles.add(realpathSync(config.configFile));
    for(const file of config.configFileDependencies||[])configFiles.add(realpathSync(file));
  },transform(code,id) {
    let file=id.split("?")[0];
    if(existsSync(file))file=realpathSync(file);
    const entry=inventory.get(file);
    if(!entry||phase==="plain")return;
    if(code!==entry.source) {
      infrastructureFailure=failure("EC_MUTATION_INSTRUMENTATION",`Source was transformed before mutation instrumentation: ${rel(file)}`);
      throw infrastructureFailure;
    }
    return entry.emitted;
  }};
}
const childDependencies=new Map();
function childProcessClosure(entries) {
  const pending=entries.map(file=>resolve(root,file)),seen=new Set();
  while(pending.length) {
    const file=pending.pop();if(seen.has(file))continue;seen.add(file);
    remember(file);
    let dependencies=childDependencies.get(file);
    if(!dependencies) {
      dependencies=[];
      const source=inventory.get(file)?.source??readFileSync(file,'utf8');
      for(const specifier of native(source,rel(file),true,{},true).metadata?.dependencies||[]) {
        if(!specifier.startsWith('.'))continue;
        const target=resolve(dirname(file),specifier);
        const candidates=[target,...['.ts','.tsx','.mts','.cts','.js','.jsx','.mjs','.cjs'].map(ext=>target+ext),...['index.ts','index.js','index.mjs','index.cjs'].map(name=>join(target,name))];
        const found=candidates.find(path=>existsSync(path)&&statSync(path).isFile());
        if(found&&/[.][cm]?[jt]sx?$/.test(found)&&!rel(found).startsWith('../')&&!rel(found).split('/').includes('node_modules'))dependencies.push(found);
      }
      childDependencies.set(file,dependencies);
    }
    pending.push(...dependencies);
  }
  return [...seen].map(rel).sort();
}
function childProcessEntries(fact) {
  const tokens=fact.method==='execSync'||fact.argv[1]==='-c'
    ?(fact.argv.at(-1).match(/(?:[^\s"'\\]|\\.|"[^"]*"|'[^']*')+/g)||[]).map(v=>v.replace(/^(["'])([\s\S]*)\1$/,'$2').replace(/\\([\s"'])/g,'$1'))
    :fact.argv.slice(1);
  const entries=new Set();
  for(const arg of tokens) {
    if(!arg||arg.startsWith('-')||!/[.][cm]?[jt]sx?$/.test(arg))continue;
    const path=resolve(fact.cwd,arg),relative=rel(path);
    if(relative.startsWith('../')||relative.split('/').includes('node_modules')||!existsSync(path))continue;
    if(!statSync(path).isFile())continue;
    entries.add(relative);
  }
  return childProcessClosure([...entries]);
}
function recordChildProcesses(execution,moduleFile,facts) {
  for(const fact of facts) {
    const entries=childProcessEntries(fact);
    if(!entries.length)continue;
    const test=execution.tests.find(t=>t.file===moduleFile&&t.taskId===fact.taskId&&t.started)?.id||null;
    (execution.childProcesses||=[]).push({test,file:moduleFile,executable:basename(fact.argv[0]),entries,method:fact.method});
  }
}
function recordModule(module,execution,request) {
  const counts=new Map();
  const moduleFile=rel(module.moduleId);
  const duration=module.diagnostic();
  execution.moduleDurations[moduleFile]=duration.duration+duration.collectDuration+duration.setupDuration+duration.environmentSetupDuration+duration.prepareDuration;
  for(const test of module.children.allTests()) {
    const occurrence=counts.get(test.fullName)||0;counts.set(test.fullName,occurrence+1);
    const id=JSON.stringify([projectName,moduleFile,test.fullName,occurrence]);
    const meta=test.meta().__errspecMutation;
    if(meta && (meta.runId!==runId||meta.requestId!==request.requestId))
      throw failure("EC_MUTATION_PROTOCOL","Stale test metadata");
    const result=test.result();
    if(options.killMatrix&&(test.diagnostic()?.retryCount>0||test.diagnostic()?.repeatCount>0))
      execution.acquisitionIncomplete=`Complete acquisition included a retried or repeated test: ${id}`;
    execution.tests.push({id,taskId:test.id,file:moduleFile,name:test.fullName,status:result.state,
      started:request.started.has(test.id),duration:test.diagnostic()?.duration||0,
      errors:(result.errors||[]).map(e=>`${e.name||"Error"}: ${e.message||e}`),hits:meta?.hits||{},infected:Object.keys(meta?.infected||{}),
    });
  }
  const meta=module.meta().__errspecMutation;
  if(!meta&&execution.tests.some(t=>t.file===moduleFile&&t.started))
    throw failure("EC_MUTATION_PROTOCOL","Test worker did not install the mutation runtime");
  if(meta) {
    if(options.weakMutation&&meta.weakVersion!==1)throw failure("EC_MUTATION_PROTOCOL","Worker does not support weak mutation");
    if(meta.runId!==runId||meta.requestId!==request.requestId) throw failure("EC_MUTATION_PROTOCOL","Stale module metadata");
    recordChildProcesses(execution,moduleFile,meta.childProcesses||[]);
    for(const [key,count] of Object.entries(meta.hits))execution.hits[key]=(execution.hits[key]||0)+count;
    for(const key of Object.keys(meta.staticHits)) (execution.staticHits[key]||=[]).push(moduleFile);
  }
  if(module.errors().length)execution.failedModules.push(moduleFile);
  for(const e of module.errors())execution.errors.push(`${e.name||"Error"}: ${e.message||e}`);
  for(const suite of module.children.allSuites())for(const e of suite.errors()) {
    if(!execution.failedModules.includes(moduleFile))execution.failedModules.push(moduleFile);
    execution.errors.push(`${e.name||"Error"}: ${e.message||e}`);
  }
}
async function closeContext(ctx) {
  process.exitCode=0;
  await ctx.close();
  const failed=process.exitCode;
  process.exitCode=0;
  if(failed)throw failure("EC_MUTATION_RUNNER","Vitest cleanup reported a failure; see stderr");
}
async function executeAdapter(phase,request) {
  verifyInputs();
  const inspected=await adapter.execute("inspect",request,inventory,baseline);
  const specs=inspected.specs;
  const excluded=new Set([...inspected.excluded,...mutationPlugins.files,...configFiles].filter(f=>existsSync(f)).map(f=>realpathSync(f)));
  for(const file of excluded)remember(file);
  const files=candidateFiles(excluded);
  if(phase==="plain") {
    registerSources(inventory,files,remember);
    testSpecs=specs;
    if(options.plugins?.length)await generatePluginSources(inventory,generateSource,rel);
    for(const file of mutationPlugins.files)inventory.delete(file);
    // Adapters revalidate conservatively: all local source/test/config inputs
    // are immutable for this run, and unknown external inputs disable caching.
    for(const file of globSync("**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs,json,snap,html,css,scss,sass,less,svg,png,jpg,jpeg,webp,avif,gif,wasm,txt,md}",{cwd:root,exclude:["**/node_modules/**","**/.git/**"]})) {
      const absolute=resolve(root,file);
      if(!absolute.startsWith(toolTemp+"/")&&!absolute.includes("/.errspec/"))remember(absolute);
    }
    for(const file of globSync("**/.env*",{cwd:root,exclude:["**/node_modules/**","**/.git/**"]}))remember(resolve(root,file));
    rememberProjectManifests(root,remember);
  } else if(JSON.stringify(specs)!==JSON.stringify(testSpecs)||JSON.stringify(files)!==JSON.stringify([...inventory.keys()]))
    throw failure("EC_MUTATION_INPUT","Source or test file inventory changed during the run");
  if(phase==="inspect"){verifyInputs();return;}
  const result=await adapter.execute(phase,request,inventory,baseline);
  verifyInputs();
  return result.execution;
}
async function execute(phase,request) {
  if(adapter)return executeAdapter(phase,request);
  verifyInputs();
  if(phase==="plain"&&warmContext){const saved=warmContext;warmContext=undefined;await closeContext(saved.ctx);}
  infrastructureFailure=undefined;
  const execution={selected:request.selected||[],tests:[],failedModules:[],errors:[],staticHits:{},hits:{},wallTimeout:false,moduleDurations:{}};
  request.started=new Set();
  const previous=phase!=="plain"?warmContext:undefined;
  if(previous?.state.failure)throw previous.state.failure;
  warmContext=undefined;
  const state=previous?.state||{};
  Object.assign(state,{request,execution,active:true});
  const active=()=>{
    if(!state.active){state.failure=failure("EC_MUTATION_PROTOCOL","Runner emitted an event outside its execution request");return false;}
    return true;
  };
  let ctx, completed=false, reusable=false, timeouts=previous?.timeouts;
  const report = {
    onTestCaseReady(test){
      if(!active())return;
      if(!test.diagnostic()?.startTime)return;
      state.request.started.add(test.id); send(state.request,"progress",{event:"testStart"});
    },
    onTestModuleEnd(module){if(active())try{recordModule(module,state.execution,state.request);}catch(error){state.failure=error;}},
    onTestRunEnd(_modules,errors){if(active())for(const e of errors)state.execution.errors.push(`${e.name||"Error"}: ${e.message||e}`);},
    onUserConsoleLog(log){process.stderr.write(log.content);return false;},
  };
  try {
    ctx=previous?.ctx||await api.createVitest({root,config:options.vitestConfig||undefined,
      project:options.vitestProject?[options.vitestProject]:undefined,
      watch:false,update:false,coverage:{enabled:false},reporters:[report],
      pool:"forks",isolate:true,maxWorkers:1,fileParallelism:false,
      typecheck:{enabled:false,only:false},
      cache:false,fsModuleCache:false,allowOnly:false,passWithNoTests:false,
    },{plugins:[plugin(phase)],server:{fs:{allow:[root,options.temp,packageDir]}}});
    current=ctx;
    const projects=ctx.projects.filter(p=>!p.config.typecheck?.only);
    if(projects.length!==1)throw failure("EC_MUTATION_CONFIG",`Select exactly one Vitest project with --vitest-project: ${projects.map(p=>p.config.name||"<unnamed>").join(", ")}`);
    const project=projects[0];projectName=project.config.name||"";
    if(project.config.browser?.enabled)throw failure("EC_MUTATION_CONFIG","Vitest Browser Mode is not supported by mutate");
    const config=project.config;
    const rootProject=typeof ctx.getRootProject==="function"?ctx.getRootProject():undefined;
    if(options.reuseSession&&phase!=="plain") {
      const globalSetup=[...ctx.projects,...(rootProject?[rootProject]:[])].some(p=>p.config.globalSetup?.length);
      reuseMode=globalSetup?"fresh_global_setup":rootProject&&typeof ctx.clearSpecificationsCache==="function"?"available":"fresh_lifecycle_unknown";
      reusable=reuseMode==="available";
    }
    timeouts??={test:config.testTimeout,hook:config.hookTimeout,retry:config.retry,bail:config.bail};
    if(options.cache){
      try {
        if([...ctx.projects,...(rootProject?[rootProject]:[])].some(p=>p.config.globalSetup?.length))throw new Error("globalSetup may provide external inputs");
        const provided={...project.getProvidedContext()};delete provided.__errspecMutation;
        const sequence={...config.sequence};delete sequence.sequencer;if(!sequence.shuffle)delete sequence.seed;
        if(!mutationPlugins.cacheable)throw new Error("Plugin has no deterministic input contract");
        configurationHash=hash(serialize({compiler:compilerInputs(project.vite,config,relocatedPath),provided,env:config.env,include:config.include,exclude:config.exclude,
          testNamePattern:config.testNamePattern,testTimeout:timeouts.test,hookTimeout:timeouts.hook,
          retry:options.killMatrix?0:config.retry,bail:options.killMatrix?0:config.bail,sequence,setupFiles:config.setupFiles.filter(f=>f!==options.setup).map(relocatedPath),
          globalSetup:config.globalSetup,environment:config.environment,environmentOptions:config.environmentOptions,
          restoreMocks:config.restoreMocks,clearMocks:config.clearMocks,mockReset:config.mockReset,
          unstubEnvs:config.unstubEnvs,unstubGlobals:config.unstubGlobals,expect:config.expect,
          fakeTimers:config.fakeTimers,chaiConfig:config.chaiConfig}));
      }catch{configurationHash=null;}
    }
    config.watch=false;config.update=false;config.snapshotOptions={...config.snapshotOptions,updateSnapshot:"none"};
    if(options.killMatrix || request.complete){config.bail=0;config.retry=0;ctx.config.bail=0;ctx.config.retry=0;}
    ctx.config.snapshotOptions.updateSnapshot="none";ctx.snapshot.options.updateSnapshot="none";
    config.pool="forks";config.isolate=true;config.maxWorkers=1;config.fileParallelism=false;config.fsModuleCache=false;
    config.typecheck={...config.typecheck,enabled:false,only:false};
    config.coverage={...config.coverage,enabled:false};
    config.setupFiles=[options.setup,...config.setupFiles.filter(f=>f!==options.setup)];
    config.provide={...config.provide,__errspecMutation:{runId,requestId:request.requestId,
      activeKey:request.key||null,hitLimit:request.hitLimit||Number.MAX_SAFE_INTEGER,weakMutation:options.weakMutation}};
    project.provide("__errspecMutation",config.provide.__errspecMutation);
    config.retry=request.complete?0:timeouts.retry;config.bail=request.complete?0:timeouts.bail;
    config.testTimeout=timeouts.test*(request.multiplier||1);
    config.hookTimeout=timeouts.hook*(request.multiplier||1);
    if(!previous)await ctx.standalone();
    // Re-enumerate files and obtain fresh specification objects. Previous testIds
    // and discovery caches must not constrain the next mutation's selection.
    if(typeof ctx.clearSpecificationsCache==="function")ctx.clearSpecificationsCache();
    const specs=await ctx.globTestSpecifications();
    const excluded=new Set([...mutationPlugins.files,...configFiles,...config.setupFiles,...(config.globalSetup||[]),...specs.map(s=>s.moduleId)]
      .filter(f=>existsSync(f)).map(f=>realpathSync(f)));
    for(const file of globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}",{cwd:root,exclude:["**/node_modules/**"]}))excluded.add(realpathSync(resolve(root,file)));
    for(const file of excluded)remember(file);
    // Track all enumerated inputs before the plain baseline, including unimported files.
    if(phase==="plain") {
      const files=candidateFiles(excluded);
      registerSources(inventory,files,remember);
      testSpecs=specIdentities(specs);
      for(const file of [...globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}",{cwd:root,exclude:["**/node_modules/**"]})])remember(resolve(root,file));
      for(const file of globSync("**/*.snap",{cwd:root,exclude:["**/node_modules/**"]}))remember(resolve(root,file));
      rememberProjectManifests(root,remember);
      // Plugin data must be observed before tests can modify it. Retain the
      // generated inventory so callbacks run once per source, including cache hits.
      if(options.plugins?.length)await generatePluginSources(inventory,generateSource,rel);
      // Async visitors may reveal additional plugin modules inside include globs.
      for(const file of mutationPlugins.files)inventory.delete(file);
    } else {
      const currentSpecs=specIdentities(specs);
      if(JSON.stringify(currentSpecs)!==JSON.stringify(testSpecs)
        ||JSON.stringify(candidateFiles(excluded))!==JSON.stringify([...inventory.keys()]))
        throw failure("EC_MUTATION_INPUT","Source or test file inventory changed during the run");
    }
    let planned=specs;
    if(execution.selected.length) {
      const grouped=new Map();
      for(const id of execution.selected) {
        const base=baseline.tests.find(t=>t.id===id);
        if(!base)throw failure("EC_MUTATION_PROTOCOL",`Unknown test ID: ${id}`);
        const file=resolve(root,base.file);(grouped.get(file)||grouped.set(file,[]).get(file)).push(base.taskId);
      }
      planned=specs.filter(s=>grouped.has(s.moduleId));
      for(const spec of planned)spec.testIds=grouped.get(spec.moduleId);
    }
    if(phase!=="inspect")await ctx.runTestSpecifications(planned,true);
    if(state.failure)throw state.failure;
    if(infrastructureFailure)throw infrastructureFailure;
    if(phase==="plain"&&options.experimentalCallgraph&&!execution.failedModules.length&&!execution.errors.length
       &&execution.tests.some(test=>test.started&&test.status==="passed")&&!execution.tests.some(test=>test.status==="failed"))
      await acquireStaticImports(project);
    // Include local Vite dependencies, not just mutation targets.
    for(const project of ctx.projects)for(const environment of Object.values(project.vite.environments))
      for(const module of environment.moduleGraph?.idToModuleMap?.values()||[]) {
        const file=module.file;
        if(file&&!file.includes("/node_modules/")&&!file.startsWith(options.temp)&&existsSync(file))remember(file);
      }
    verifyInputs();
    execution.tests.sort((a,b)=>a.file<b.file?-1:a.file>b.file?1:0); // retain registration order within each module
    completed=true;
    return execution;
  } catch(error) {
    if(!ctx&&!error.code?.startsWith("EC_MUTATION_"))throw failure(phase==="plain"?"EC_MUTATION_CONFIG":"EC_MUTATION_RUNNER",error.stack||String(error));
    throw error;
  } finally {
    state.active=false;
    if(ctx&&completed&&reusable&&!state.failure)warmContext={ctx,state,timeouts};
    else if(ctx)await closeContext(ctx);
    current=undefined;process.exitCode=0;
  }
}
function send(request,type,payload={}) {
  protocolWrite(JSON.stringify({version:PROTOCOL,runId,requestId:request.requestId,type,...payload})+"\n");
}
function cacheFacts(){
  return {version:1,inputs:[...fingerprints],runnerPackage:packageDir,runtime:{node:process.versions.node,platform:process.platform,arch:process.arch,executable:process.execPath,cwd:process.cwd(),execArgv:process.execArgv,configurationHash,envHash:hash(JSON.stringify(Object.entries(process.env).sort(([a],[b])=>a<b?-1:a>b?1:0).map(([name,value])=>[name,options.cacheRelocation&&['PWD','INIT_CWD'].includes(name)?relocatedPath(value):value])))}};
}
async function acquireStaticImports(project) {
  const container=project.vite.environments?.__vitest__?.pluginContainer ?? project.vite.environments?.ssr?.pluginContainer;
  for(const [file,entry] of inventory) {
    const generated=entry.generated ?? await generateSource(entry.source,rel(file),native(entry.source,rel(file)));
    pendingGenerated.set(file,generated);
    for(const binding of generated.metadata?.imports||[]) {
      binding.resolved=null;
      try {
        const resolved=await container?.resolveId(binding.source,file);
        let target=resolved?.id?.split('?')[0];
        if(target?.startsWith('/@fs/'))target=target.slice(4);
        if(!target||!isAbsolute(target)||!existsSync(target))continue;
        target=normalize(realpathSync(target));remember(target);
        if(inventory.has(target))binding.resolved=rel(target);
      } catch { /* Unknown resolution keeps callers opaque in the native graph. */ }
    }
  }
}
async function handle(request) {
  if(request.version!==PROTOCOL||typeof request.runId!=="string"||!Number.isSafeInteger(request.requestId))
    throw failure("EC_MUTATION_PROTOCOL","Invalid request envelope");
  if(runId&&runId!==request.runId)throw failure("EC_MUTATION_PROTOCOL","Wrong run ID");
  runId=request.runId;
  if(request.op==="open") {
    options=request.options;
    pendingGenerated.clear();
    if(Number(process.versions.node.split(".")[0])<24)throw failure("EC_MUTATION_CONFIG","mutate requires Node 24 or newer");
    root=realpathSync(options.root);toolTemp=realpathSync(options.toolTemp||options.temp);
    mutationPlugins=await loadPlugins(options.plugins,root,remember,options.excludeMutator);
    try{runner=detectRunner(root,options);}catch(error){throw failure("EC_MUTATION_CONFIG",error.message);}
    if(!["vitest","vite-plus"].includes(runner)&&(options.vitestConfig||options.vitestPackage||options.vitestProject))throw failure("EC_MUTATION_CONFIG","Vitest options do not match the selected runner");
    options.vitestConfig??=options.runnerConfig;options.vitestProject??=options.runnerProject;
    const packageName={vitest:"vitest","vite-plus":"vite-plus",jest:"jest",playwright:"@playwright/test"}[runner];
    try{packageDir=resolvePackage(root,packageName,options.runnerPackage||options.vitestPackage);}catch(error){throw failure("EC_MUTATION_CONFIG",error.message);}
    if(!packageDir)throw failure("EC_MUTATION_CONFIG",`Cannot find ${packageName}; use --runner-package`);
    const runnerVersion=JSON.parse(readFileSync(join(packageDir,"package.json"),"utf8")).version;
    if(runner==="vite-plus") {
      remember(join(packageDir,"package.json"));
      const vitest=resolvePackage(packageDir,"vitest");
      if(!vitest)throw failure("EC_MUTATION_CONFIG","Vite+ must expose a compatible Vitest 5 installation");
      packageDir=vitest;
    }
    const pkg=JSON.parse(readFileSync(join(packageDir,"package.json"),"utf8"));
    remember(join(packageDir,"package.json"));
    if(["vitest","vite-plus"].includes(runner)){
      if(!/^5\./.test(pkg.version))throw failure("EC_MUTATION_CONFIG",`mutate requires Vitest 5.x; found ${pkg.version}`);
      api=await import(pathToFileURL(join(packageDir,"dist/node.js")).href);
      const setup=readFileSync(options.setup,"utf8").replace('"__VITEST_ENTRY__"',JSON.stringify(normalize(join(packageDir,"dist/index.js"))));
      if(setup!==readFileSync(options.setup,"utf8"))writeFileSync(options.setup,setup);
    }else if(runner==="jest"){
      if(!/^30\./.test(pkg.version))throw failure("EC_MUTATION_CONFIG",`mutate requires Jest 30.x native ESM; found ${pkg.version}`);
      adapter=await createJestAdapter({root,options,packageDir,remember,failure,rel,runId,recordChildProcesses,progress:request=>send(request,"progress",{event:"testStart"})});
    }else {
      if(!/^1\./.test(pkg.version))throw failure("EC_MUTATION_CONFIG",`mutate requires Playwright Test 1.x; found ${pkg.version}`);
      adapter=await createPlaywrightAdapter({root,options,packageDir,remember,failure,rel,runId,inventory,recordChildProcesses,progress:request=>send(request,"progress",{event:"testStart"})});
    }
    if(request.restore) {
      const saved=JSON.parse(readFileSync(options.state,"utf8"));
      baseline=saved.baseline;plain=saved.plain;testSpecs=saved.testSpecs;projectName=saved.projectName;
      const restoredInputs=new Map(saved.fingerprints);
      for(const [file,digest] of fingerprints)if(restoredInputs.get(file)!==digest)
        throw failure("EC_MUTATION_INPUT",`Input closure changed while restoring worker: ${file}`);
      inventory=new Map(saved.inventory);fingerprints=restoredInputs;verifyInputs();
      for(const [file,entry] of inventory)entry.emitted=instrument(entry.source,entry.candidates,file,runId);
    }
    send(request,"done",{mutators:mutationPlugins.names,capabilities:{complete:true},node:process.versions.node,...(["vitest","vite-plus"].includes(runner)?{vitest:pkg.version}:{}),...(runner!=="vitest"?{runner,runnerVersion}:{}),count:0});return;
  }
  if(!options)throw failure("EC_MUTATION_PROTOCOL","Driver was not opened");
  if(request.op==="prime"){
    if(!baseline)throw failure("EC_MUTATION_PROTOCOL","Missing restored worker baseline");
    const measured=await execute("probe",request);baseline=measured;
    send(request,"primed",{execution:measured,...(options.cache?{cacheFacts:cacheFacts()}:{})});send(request,"done",{count:0});return;
  }
  if(request.op==="cache-facts"){
    if(!options.cache)throw failure("EC_MUTATION_PROTOCOL","Result caching was not requested");
    verifyInputs();send(request,"done",{count:0,cacheFacts:cacheFacts()});return;
  }
  if(request.op==="prepare"||request.op==="discover") {
    plain=await execute("plain",request);
    send(request,"baseline",{execution:plain});
    if(plain.failedModules.length||plain.errors.length||!plain.tests.some(t=>t.started&&t.status==="passed")||plain.tests.some(t=>t.status==="failed"))
      throw failure("EC_MUTATION_BASELINE","Unmodified baseline failed or executed no tests");
    let count=0;
    for(const [file,entry] of inventory) {
      const generated=pendingGenerated.get(file) ?? entry.generated ?? await generateSource(entry.source,rel(file));
      pendingGenerated.delete(file);
      delete entry.generated;
      entry.candidates=generated.candidates;
      entry.critical=generated.critical||[];
      for(const c of entry.candidates)if(c.regex) {try{new RegExp(...c.regex);}catch{c.ignored="Regex replacement is invalid for the original flags";}}
      entry.emitted=instrument(entry.source,entry.candidates,file,runId);
      try { native(entry.emitted.code,rel(file),true); }
      catch(error) {
        // Reject an unsafe placement individually; valid rules in this file still run.
        for(const c of entry.candidates.filter(c=>!c.ignored)) {
          try {native(instrument(entry.source,[c],file,runId).code,rel(file),true);}
          catch {c.ignored="Switch placement is not valid in this syntax context";}
        }
        entry.emitted=instrument(entry.source,entry.candidates,file,runId);
        native(entry.emitted.code,rel(file),true);
      }
      // Entry facts describe guards actually present in the accepted switch.
      // A placement rejected above supplies no entry observation.
      if(generated.metadata)for(const scope of Object.values(generated.metadata.scopes))
        if(scope.entryKey&&!entry.candidates.some(c=>c.key===scope.entryKey&&!c.ignored))delete scope.entryKey;
      const critical=options.policy?.critical.some(pattern=>matchesGlob(rel(file),pattern))?entry.candidates.map(c=>c.key):entry.critical;
      send(request,"file",{file:rel(file),candidates:entry.candidates,critical,...(generated.metadata?{metadata:generated.metadata}: {})});count++;
    }
    if(request.op==="discover"){
      send(request,"discovered",{cacheFacts:cacheFacts()});send(request,"done",{count});return;
    }
    await prepareProbe(request,count);return;
  }
  if(request.op==="probe"){
    if(!plain||baseline)throw failure("EC_MUTATION_PROTOCOL","Probe requires an unprepared current baseline");
    await prepareProbe(request,0);return;
  }
  if(request.op==="run") {
    if(!baseline)throw failure("EC_MUTATION_PROTOCOL","Missing baseline");
    if(![...inventory.values()].some(entry=>entry.candidates.some(c=>c.key===request.key&&!c.ignored)))
      throw failure("EC_MUTATION_PROTOCOL","Unknown or ignored active mutant key");
    const execution=await execute("mutant",request);
    send(request,"attempt",{key:request.key,execution});send(request,"done",{count:1});return;
  }
  if(request.op==="typecheck") {
    if(!baseline)throw failure("EC_MUTATION_PROTOCOL","Missing baseline");
    verifyInputs();
    try {
      const keys=new Set(request.keys);
      const selected=new Map([...inventory].map(([file,entry])=>[file,{...entry,candidates:entry.candidates.filter(c=>keys.has(c.key))}]));
      const result=await typecheckMutants(options,selected,key=>send(request,"progress",{event:"typecheck",key}));
      verifyInputs();
      send(request,"typechecked",result);send(request,"done",{count:result.outcomes.length});return;
    } catch(error) { throw failure("EC_MUTATION_TYPECHECK",error.stack||String(error)); }
  }
  if(request.op==="close") {
    verifyInputs();
    // A fully reused result may run no trials. Refresh discovery anyway so a
    // newly added source or test cannot disappear behind cached coverage.
    if((options.cache||options.concurrency>1||options.killMatrix||options.selectionOnly)&&baseline)await execute("inspect",request);
    if(warmContext){const saved=warmContext;warmContext=undefined;await closeContext(saved.ctx);if(saved.state.failure)throw saved.state.failure;}
    verifyInputs();send(request,"done",{count:0,...(options.cache?{cacheFacts:cacheFacts()}:{})});return;
  }
  throw failure("EC_MUTATION_PROTOCOL",`Unknown operation: ${request.op}`);
}
async function prepareProbe(request,count){
    if(request.coverage){
      baseline=request.coverage;
      await execute("inspect",request);
    }else baseline=await execute("probe",request);
    const identity=e=>e.tests.map(t=>[t.id,t.status,t.started]);
    if(JSON.stringify(identity(plain))!==JSON.stringify(identity(baseline))||baseline.errors.length||baseline.failedModules.length)
      throw failure("EC_MUTATION_INSTRUMENTATION","Probe baseline changed test registration or results");
    writeFileSync(options.state,JSON.stringify({baseline,plain,testSpecs,projectName,inventory:[...inventory],fingerprints:[...fingerprints]}));
    send(request,"prepared",{execution:baseline,...(options.reuseSession?{sessionReuse:reuseMode}:{}),...(options.cache?{cacheFacts:cacheFacts()}:{})});send(request,"done",{count});return;
}
const lines=createInterface({input:process.stdin,crlfDelay:Infinity});
for await(const line of lines) {
  let request;
  try {request=JSON.parse(line);await handle(request);}
  catch(error) {
    try {
      if(current)await closeContext(current);
      if(warmContext){const saved=warmContext;warmContext=undefined;await closeContext(saved.ctx);}
    }catch(cleanup){error=failure(error.code||"EC_MUTATION_RUNNER",`${error.stack||error}\nCleanup failed: ${cleanup.stack||cleanup}`);}
    send(request||{requestId:0},"error",{code:error.code?.startsWith("EC_MUTATION_")?error.code:"EC_MUTATION_RUNNER",message:error.stack||String(error)});
    process.exitCode=0;
  }
}
if(warmContext)await closeContext(warmContext.ctx);
