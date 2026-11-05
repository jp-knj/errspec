import { readFileSync, writeFileSync, existsSync, mkdirSync, rmSync, realpathSync } from "node:fs";
import { join, resolve } from "node:path";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { runChild } from "./process-runner.mjs";
// These helpers execute inside the project's Jest process. They acquire facts;
// native Rust alone decides whether a mutation was detected.
function transformer(load) {
  const {readFileSync}=load("node:fs");
  const {stripTypeScriptTypes}=load("node:module");
  function prepare(source,file,options){
    const state=JSON.parse(readFileSync(process.env.ERRSPEC_JEST_STATE,"utf8"));
    if(file.startsWith(state.toolTemp+"/"))return {code:source};
    if(!options.supportsStaticESM)throw new Error(`errspec Jest requires native ESM: ${file}`);
    const entry=state.inventory[file];
    if(entry&&entry.source!==source)throw new Error(`Mutation source changed: ${file}`);
    const code=entry?.code??source;
    const transform=state.transforms.find(([pattern])=>new RegExp(pattern).test(file));
    return {code,transform};
  }
  const plain=(code,file)=>({code:/\.[cm]?tsx?$/.test(file)?stripTypeScriptTypes(code,{mode:"transform",sourceUrl:file}):code});
  const instances=new Map();
  function instance(transform){
    const key=JSON.stringify(transform);
    if(!instances.has(key))instances.set(key,(async()=>{
      const module=await import(load("node:url").pathToFileURL(transform[1]).href),adapter=module.default||module;
      return typeof adapter.createTransformer==="function"?await adapter.createTransformer(transform[2]):adapter;
    })());
    return instances.get(key);
  }
  return {
    async processAsync(source,file,options){
      const {code,transform}=prepare(source,file,options);
      if(!transform)return plain(code,file);
      const adapter=await instance(transform);
      const process=adapter.processAsync||adapter.process;
      if(typeof process!=="function")throw new Error(`Invalid Jest ESM transformer: ${transform[1]}`);
      return await process.call(adapter,code,file,{...options,transformerConfig:transform[2]});
    },
  };
}
function environment(Base,attachWeakBinary,load) {
  const {readFileSync,writeFileSync}=load("node:fs");
  const {join,relative}=load("node:path");
  const {createHash}=load("node:crypto");
  return class extends Base {
    constructor(config,context){super(config,context);this.file=context.testPath;}
    async setup(){
      await super.setup();
      this.state=JSON.parse(readFileSync(process.env.ERRSPEC_JEST_STATE,"utf8"));
      this.data={runId:this.state.context.runId,requestId:this.state.context.requestId,phase:this.state.phase,file:this.file,tests:[],hits:{},staticHits:{}};
      const self=this,ctx=this.state.context;
      const record=this.global[Symbol.for(`errspec.mutation.${ctx.runId}`)]=key=>{
        const count=self.data.hits[key]=(self.data.hits[key]||0)+1;
        if(self.active&&!self.concurrent){self.active.hits[key]=(self.active.hits[key]||0)+1;}
        else self.data.staticHits[key]=true;
        if(key===ctx.activeKey&&count>ctx.hitLimit){const e=new Error("ERRSPEC_HIT_LIMIT");e.name="ErrspecHitLimitError";throw e;}
        return key===ctx.activeKey;
      };
      if(ctx.weakMutation)attachWeakBinary(record,key=>{if(self.active&&!self.concurrent)self.active.infected[key]=true;},ctx.activeKey);
    }
    async handleTestEvent(event,state){
      if(super.handleTestEvent)await super.handleTestEvent(event,state);
      const t=event.test;
      if(event.name==="run_start"){
        if(this.state.context.complete){
          this.global[Symbol.for("RETRY_TIMES")]=0;
        }
        const counts=new Map(),selected=this.state.selected;
        const file=relative(this.state.root,this.file).replaceAll("\\","/");
        const walk=node=>{if(this.state.context.complete)state.describeRetryOptions?.delete(node);for(const child of node.children||[]){if(child.type==="test"){
          const names=[];for(let n=child;n&&n.name!=="ROOT_DESCRIBE_BLOCK";n=n.parent)names.unshift(n.name);
          const name=names.join(" "),occurrence=counts.get(name)||0;counts.set(name,occurrence+1);
          child.errspec={id:JSON.stringify(["",file,name,occurrence]),order:counts.size};
          if(selected.length&&!selected.includes(child.errspec.id))child.mode="skip";
          if(child.concurrent)this.concurrent=true;
        }else walk(child);}};walk(state.rootDescribeBlock);
      }
      if(event.name==="test_start"){
        this.active={test:t,hits:{},infected:{}};
        if(process.send)process.send({errspec:"testStart"});
      }
      if(["test_done","test_skip","test_todo"].includes(event.name)){
        const names=[];for(let n=t;n&&n.name!=="ROOT_DESCRIBE_BLOCK";n=n.parent)names.unshift(n.name);
        const meta=this.active?.test===t?this.active:{hits:{},infected:{}};
        if(this.state.context.complete&&(t.invocations>1||t.repeatCount>0))this.data.acquisitionIncomplete="Jest repeated or retried a selected test";
        const result={id:t.errspec.id,order:t.errspec.order,name:names.join(" "),status:event.name==="test_skip"?"skipped":event.name==="test_todo"?"pending":t.errors.length?"failed":"passed",started:event.name==="test_done",duration:t.duration||0,errors:t.errors.map(e=>String(e?.[0]?.stack||e?.stack||e)),hits:meta.hits,infected:Object.keys(meta.infected)};
        const index=this.data.tests.findIndex(t=>t.id===result.id);
        if(index>=0)this.data.tests[index]=result;else this.data.tests.push(result);
        this.active=undefined;
      }
    }
    async teardown(){
      try{writeFileSync(join(this.state.output,createHash("sha256").update(this.file).digest("hex")+".json"),JSON.stringify(this.data));}
      finally{await super.teardown();}
    }
  };
}
export async function createJestAdapter(ctx) {
  const {root,options,packageDir,remember,failure}=ctx;
  const fromJest=createRequire(join(packageDir,"package.json"));
  const configApi=createRequire(fromJest.resolve("jest-cli/package.json"))("jest-config");
  const {config:raw,configPath}=await configApi.readInitialOptions(options.runnerConfig||undefined,{packageRootOrConfig:options.runnerConfig||root});
  if(raw.projects)throw failure("EC_MUTATION_CONFIG","Jest projects are not supported; select a native ESM project");
  const {options:normalized}=await configApi.normalize(raw,{});
  if(options.runnerProject)throw failure("EC_MUTATION_CONFIG","runnerProject is not supported by the Jest native ESM adapter");
  const directory=join(options.temp,"jest");mkdirSync(directory,{recursive:true});
  const transformFile=join(directory,"transform.cjs"),environmentFile=join(directory,"environment.cjs"),configFile=join(directory,"config.json");
  writeFileSync(transformFile,`module.exports=(${transformer.toString()})(require);`);
  const weak=readFileSync(join(options.toolTemp,"weak-runtime.mjs"),"utf8").replace("export function attachWeakBinary","function attachWeakBinary");
  writeFileSync(environmentFile,`${weak}\nconst base=require(${JSON.stringify(normalized.testEnvironment)});module.exports=(${environment.toString()})(base.TestEnvironment||base.default||base,attachWeakBinary,require);`);
  const config={...raw,rootDir:normalized.rootDir,testEnvironment:environmentFile,
    transform:{"\\.[cm]?[jt]sx?$":transformFile},extensionsToTreatAsEsm:[...new Set([...normalized.extensionsToTreatAsEsm,".ts",".mts"])],
    transformIgnorePatterns:[...normalized.transformIgnorePatterns,realpathSync(options.temp).replace(/[.*+?^${}()|[\]\\]/g,"\\$&")+"/"],
    cache:false,cacheDirectory:join(options.temp,"jest-cache"),watch:false,watchman:false,collectCoverage:false,updateSnapshot:false,reporters:[],maxWorkers:1};
  writeFileSync(configFile,JSON.stringify(config));
  const configFiles=[configPath,...normalized.setupFiles,...normalized.setupFilesAfterEnv,normalized.globalSetup,normalized.globalTeardown,...normalized.transform.map(t=>t[1])].filter(Boolean);
  configFiles.forEach(file=>remember(file));
  const cli=join(packageDir,"bin/jest.js");
  return {async execute(phase,request,inventory,baseline){
    configFiles.forEach(file=>remember(file));
    writeFileSync(configFile,JSON.stringify({...config,...(request.complete?{bail:0}:{})}));
    const list=await runChild(["--experimental-vm-modules",cli,"--config",configFile,"--listTests","--json"],{root,onProgress:()=>{},capture:true});
    if(list.code!==0)throw failure("EC_MUTATION_CONFIG","Jest test discovery failed");
    let files;try{files=JSON.parse(list.stdout).sort();}catch{throw failure("EC_MUTATION_PROTOCOL","Jest returned invalid test discovery JSON");}
    const specs=files.map(file=>({file,project:""}));
    if(phase==="inspect")return {specs,excluded:[...configFiles,...files]};
    for(const file of inventory.keys())if(normalized.transformIgnorePatterns.some(pattern=>new RegExp(pattern).test(file)))throw failure("EC_MUTATION_CONFIG",`Mutation target is excluded from Jest transforms: ${file}; exclude the target or adjust transformIgnorePatterns`);
    const output=join(directory,`results-${request.requestId}`);rmSync(output,{recursive:true,force:true});mkdirSync(output,{recursive:true});
    const stateFile=join(directory,"state.json"),reportFile=join(directory,"report.json");
    rmSync(reportFile,{force:true});
    writeFileSync(stateFile,JSON.stringify({root,phase,toolTemp:realpathSync(options.toolTemp),transforms:normalized.transform,output,selected:request.selected||[],
      context:{runId:ctx.runId,requestId:request.requestId,activeKey:request.key||null,hitLimit:request.hitLimit||Number.MAX_SAFE_INTEGER,weakMutation:options.weakMutation,complete:!!request.complete},
      inventory:Object.fromEntries([...inventory].map(([file,entry])=>[file,{source:entry.source,code:phase==="plain"?entry.source:entry.emitted.code}]))}));
    const selectedFiles=request.selected?.length?[...new Set(request.selected.map(id=>{const test=baseline.tests.find(t=>t.id===id);if(!test)throw failure("EC_MUTATION_PROTOCOL",`Unknown test ID: ${id}`);return resolve(root,test.file);}))]:files;
    const outcome=await runChild(["--experimental-vm-modules",cli,"--config",configFile,"--ci","--runInBand","--json","--outputFile",reportFile,"--testTimeout",String((normalized.testTimeout||5000)*(request.multiplier||1)),"--runTestsByPath",...selectedFiles],
      {root,env:{ERRSPEC_JEST_STATE:stateFile},onProgress:()=>ctx.progress(request)});
    if(!existsSync(reportFile))throw failure("EC_MUTATION_RUNNER",`Jest exited without results (${outcome.code??outcome.signal})`);
    const report=JSON.parse(readFileSync(reportFile,"utf8"));
    const execution={selected:request.selected||[],tests:[],failedModules:[],errors:[],hits:{},staticHits:{},moduleDurations:{},wallTimeout:false};
    for(const fileResult of report.testResults){
      const file=fileResult.name,name=ctx.rel(file),metadata=join(output,createHash("sha256").update(file).digest("hex")+".json");
      execution.moduleDurations[name]=(fileResult.endTime||0)-(fileResult.startTime||0);
      if(existsSync(metadata)){
        const data=JSON.parse(readFileSync(metadata,"utf8"));
        if(data.runId!==ctx.runId||data.requestId!==request.requestId||data.phase!==phase||data.file!==file)throw failure("EC_MUTATION_PROTOCOL",`Stale Jest worker metadata: ${name}`);
        if(data.acquisitionIncomplete)execution.acquisitionIncomplete=data.acquisitionIncomplete;
        for(const test of data.tests.sort((a,b)=>a.order-b.order)){delete test.order;execution.tests.push({...test,taskId:test.id,file:name});}
        for(const [key,count]of Object.entries(data.hits))execution.hits[key]=(execution.hits[key]||0)+count;
        for(const key of Object.keys(data.staticHits))(execution.staticHits[key]||=[]).push(name);
      }else if(fileResult.assertionResults.length)throw failure("EC_MUTATION_PROTOCOL",`Jest worker did not provide mutation metadata: ${name}`);
      if(fileResult.testExecError||fileResult.status==="failed"&&!fileResult.assertionResults.length){execution.failedModules.push(name);execution.errors.push(`Jest native ESM module failed: ${fileResult.message||name}`);}
    }
    if(report.runExecError)execution.errors.push(String(report.runExecError));
    if(![0,1].includes(outcome.code))execution.errors.push(`Jest process terminated: ${outcome.code??outcome.signal}`);
    return {execution,specs,excluded:[...configFiles,...files]};
  }};
}
