import { readFileSync, writeFileSync, realpathSync, existsSync, globSync, statSync } from "node:fs";
import { dirname, resolve, relative, join, isAbsolute, matchesGlob } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { format } from "node:util";
import { serialize } from "node:v8";
import { instrument } from "./emitter.mjs";
import { typecheckMutants } from "./typecheck.mjs";
import { compilerInputs } from "./cache-inputs.mjs";

const protocolWrite = process.stdout.write.bind(process.stdout);
// Config evaluation happens in this process, before Vitest redirects test logs.
process.stdout.write = (chunk, encoding, cb) => process.stderr.write(chunk, encoding, cb);
console.log = (...args) => process.stderr.write(format(...args)+"\n");
console.info = console.log;
const PROTOCOL = 1;
let options, root, runId, api, packageDir, toolTemp, inventory = new Map(), fingerprints = new Map();
let plain, baseline, testSpecs, projectName, configFiles = new Set();
let current;
let infrastructureFailure;
let warmContext, reuseMode="fresh_lifecycle_unknown";
let configurationHash=null;
const normalize = s => s.replaceAll("\\", "/");
const rel = file => normalize(relative(root,file));
const hash = data => createHash("sha256").update(data).digest("hex");
const relocatedPath = path => {
  if(!options.cacheRelocation||!isAbsolute(path))return path;
  const name=rel(path);
  return name!=='..'&&!name.startsWith('../')&&!isAbsolute(name) ? {projectRootRelativePath:name} : path;
};
function failure(code,message) { const e = new Error(message); e.code=code; return e; }
function remember(file) {
  if (!existsSync(file) || !statSync(file).isFile()) return;
  file=realpathSync(file);
  const next=hash(readFileSync(file));
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
function native(source,file,validateOnly=false) {
  const result=spawnSync(options.binary,["__mutation-source"], {
    input:JSON.stringify({source,file,validateOnly,excludeMutators:options.excludeMutator,weakMutation:options.weakMutation,arid:options.arid,aridCallees:options.aridCallees,facts:!!options.since,engineFingerprint:options.engineFingerprint}),
    encoding:"utf8",maxBuffer:64*1024*1024,timeout:60000,
  });
  if(result.status!==0) throw failure(result.stderr?.startsWith("EC_MUTATION_INPUT:")?"EC_MUTATION_INPUT":"EC_MUTATION_INSTRUMENTATION",result.stderr||"Native parser failed");
  try {return JSON.parse(result.stdout);} catch {throw failure("EC_MUTATION_PROTOCOL","Invalid native generator response");}
}
function packagePath(explicit) {
  if (explicit) {
    if(!existsSync(join(explicit,"package.json")))throw failure("EC_MUTATION_CONFIG",`Vitest package not found: ${explicit}`);
    return realpathSync(explicit);
  }
  for(let dir=root;;dir=dirname(dir)) {
    const candidate=join(dir,"node_modules/vitest");
    if(existsSync(join(candidate,"package.json")))return realpathSync(candidate);
    if(dirname(dir)===dir)break;
  }
  throw failure("EC_MUTATION_CONFIG","Cannot find Vitest; use --vitest-package");
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
async function execute(phase,request) {
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
    timeouts??={test:config.testTimeout,hook:config.hookTimeout};
    if(options.cache){
      try {
        if([...ctx.projects,...(rootProject?[rootProject]:[])].some(p=>p.config.globalSetup?.length))throw new Error("globalSetup may provide external inputs");
        const provided={...project.getProvidedContext()};delete provided.__errspecMutation;
        const sequence={...config.sequence};delete sequence.sequencer;if(!sequence.shuffle)delete sequence.seed;
        configurationHash=hash(serialize({compiler:compilerInputs(project.vite,config,relocatedPath),provided,env:config.env,include:config.include,exclude:config.exclude,
          testNamePattern:config.testNamePattern,testTimeout:timeouts.test,hookTimeout:timeouts.hook,
          retry:config.retry,bail:config.bail,sequence,setupFiles:config.setupFiles.filter(f=>f!==options.setup).map(relocatedPath),
          globalSetup:config.globalSetup,environment:config.environment,environmentOptions:config.environmentOptions,
          restoreMocks:config.restoreMocks,clearMocks:config.clearMocks,mockReset:config.mockReset,
          unstubEnvs:config.unstubEnvs,unstubGlobals:config.unstubGlobals,expect:config.expect,
          fakeTimers:config.fakeTimers,chaiConfig:config.chaiConfig}));
      }catch{configurationHash=null;}
    }
    config.watch=false;config.update=false;config.snapshotOptions={...config.snapshotOptions,updateSnapshot:"none"};
    ctx.config.snapshotOptions.updateSnapshot="none";ctx.snapshot.options.updateSnapshot="none";
    config.pool="forks";config.isolate=true;config.maxWorkers=1;config.fileParallelism=false;config.fsModuleCache=false;
    config.typecheck={...config.typecheck,enabled:false,only:false};
    config.coverage={...config.coverage,enabled:false};
    config.setupFiles=[options.setup,...config.setupFiles.filter(f=>f!==options.setup)];
    config.provide={...config.provide,__errspecMutation:{runId,requestId:request.requestId,
      activeKey:request.key||null,hitLimit:request.hitLimit||Number.MAX_SAFE_INTEGER,weakMutation:options.weakMutation}};
    project.provide("__errspecMutation",config.provide.__errspecMutation);
    config.testTimeout=timeouts.test*(request.multiplier||1);
    config.hookTimeout=timeouts.hook*(request.multiplier||1);
    if(!previous)await ctx.standalone();
    // Re-enumerate files and obtain fresh specification objects. Previous testIds
    // and discovery caches must not constrain the next mutation's selection.
    if(typeof ctx.clearSpecificationsCache==="function")ctx.clearSpecificationsCache();
    const specs=await ctx.globTestSpecifications();
    const excluded=new Set([...configFiles,...config.setupFiles,...(config.globalSetup||[]),...specs.map(s=>s.moduleId)]
      .filter(f=>existsSync(f)).map(f=>realpathSync(f)));
    for(const file of globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}",{cwd:root,exclude:["**/node_modules/**"]}))excluded.add(realpathSync(resolve(root,file)));
    for(const file of excluded)remember(file);
    // Track all enumerated inputs before the plain baseline, including unimported files.
    if(phase==="plain") {
      const files=candidateFiles(excluded);
      for(const file of files){const source=readFileSync(file,"utf8");inventory.set(file,{source,candidates:[]});remember(file);}
      testSpecs=specIdentities(specs);
      for(const file of [...globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}",{cwd:root,exclude:["**/node_modules/**"]})])remember(resolve(root,file));
      for(const file of globSync("**/*.snap",{cwd:root,exclude:["**/node_modules/**"]}))remember(resolve(root,file));
      for(let dir=root;;dir=dirname(dir)) {
        for(const name of ["package.json","package-lock.json","bun.lock","bun.lockb","pnpm-lock.yaml","yarn.lock","tsconfig.json"])remember(join(dir,name));
        if(dirname(dir)===dir)break;
      }
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
async function handle(request) {
  if(request.version!==PROTOCOL||typeof request.runId!=="string"||!Number.isSafeInteger(request.requestId))
    throw failure("EC_MUTATION_PROTOCOL","Invalid request envelope");
  if(runId&&runId!==request.runId)throw failure("EC_MUTATION_PROTOCOL","Wrong run ID");
  runId=request.runId;
  if(request.op==="open") {
    options=request.options;
    if(Number(process.versions.node.split(".")[0])<24)throw failure("EC_MUTATION_CONFIG","mutate requires Node 24 or newer");
    root=realpathSync(options.root);toolTemp=realpathSync(options.toolTemp||options.temp);packageDir=packagePath(options.vitestPackage);
    const pkg=JSON.parse(readFileSync(join(packageDir,"package.json"),"utf8"));
    remember(join(packageDir,"package.json"));
    if(!/^5\./.test(pkg.version))throw failure("EC_MUTATION_CONFIG",`mutate requires Vitest 5.x; found ${pkg.version}`);
    api=await import(pathToFileURL(join(packageDir,"dist/node.js")).href);
    const setup=readFileSync(options.setup,"utf8").replace('"__VITEST_ENTRY__"',JSON.stringify(normalize(join(packageDir,"dist/index.js"))));
    if(setup!==readFileSync(options.setup,"utf8"))writeFileSync(options.setup,setup);
    if(request.restore) {
      const saved=JSON.parse(readFileSync(options.state,"utf8"));
      baseline=saved.baseline;plain=saved.plain;testSpecs=saved.testSpecs;projectName=saved.projectName;
      inventory=new Map(saved.inventory);fingerprints=new Map(saved.fingerprints);verifyInputs();
      for(const [file,entry] of inventory)entry.emitted=instrument(entry.source,entry.candidates,file,runId);
    }
    send(request,"done",{node:process.versions.node,vitest:pkg.version,count:0});return;
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
      const generated=native(entry.source,rel(file));
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
      const critical=options.policy?.critical.some(pattern=>matchesGlob(rel(file),pattern))?entry.candidates.map(c=>c.key):entry.critical;
      send(request,"file",{file:rel(file),candidates:entry.candidates,critical,...(options.since?{metadata:generated.metadata}: {})});count++;
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
    if((options.cache||options.concurrency>1)&&baseline)await execute("inspect",request);
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
