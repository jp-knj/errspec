import {readFileSync,writeFileSync,existsSync,mkdirSync,realpathSync,rmSync,readdirSync} from "node:fs";
import {join,dirname,resolve} from "node:path";
import {pathToFileURL} from "node:url";
import {runChild} from "./process-runner.mjs";
import {resolvePackage} from "./runners.mjs";
function browserRuntime(context,weak){
  const counts={},pending=globalThis.__errspecMutationPending=[];
  const notify=(key,infected=false)=>{
    const promise=globalThis.__errspecMutationHit({runId:context.runId,requestId:context.requestId,key,infected});
    pending.push(promise);promise.catch(()=>{});
  };
  const record=globalThis[Symbol.for(`errspec.mutation.${context.runId}`)]=key=>{
    notify(key);
    const count=counts[key]=(counts[key]||0)+1;
    if(key===context.activeKey&&count>context.hitLimit){const error=new Error("ERRSPEC_HIT_LIMIT");error.name="ErrspecHitLimitError";throw error;}
    return key===context.activeKey;
  };
  if(context.weakMutation)weak(record,key=>notify(key,true),context.activeKey);
}
function reporter(load){
  const {readFileSync,writeFileSync}=load("node:fs");
  const {relative}=load("node:path");
  return class {
    onBegin(config,suite){
      this.state=JSON.parse(readFileSync(process.env.ERRSPEC_BROWSER_STATE,"utf8"));
      const counts=new Map();this.tests=new Map();this.errors=[];
      for(const test of suite.allTests()){
        const file=relative(this.state.root,test.location.file).replaceAll("\\","/");
        const project=test.parent.project()?.name||"";
        const names=[test.title];for(let p=test.parent;p?.type==="describe";p=p.parent)names.unshift(p.title);
        const name=names.join(" "),group=JSON.stringify([project,file,name]),occurrence=counts.get(group)||0;counts.set(group,occurrence+1);
        this.tests.set(test.id,{id:JSON.stringify([project,file,name,occurrence]),taskId:test.id,file,name,status:"skipped",started:false,duration:0,errors:[],hits:{},infected:[]});
      }
    }
    onTestBegin(){if(process.send)process.send({errspec:"testStart"});}
    onTestEnd(test,result){
      const item=this.tests.get(test.id);
      if(this.state.matrix&&(result.retry>0||test.repeatEachIndex>0))this.acquisitionIncomplete="Playwright repeated or retried a selected test";
      item.status=result.status==="skipped"?"skipped":result.status===test.expectedStatus?"passed":"failed";
      item.started=result.status!=="skipped";item.duration=result.duration;
      item.errors=item.status==="passed"?[]:result.errors.map(e=>e.stack||e.message||String(e));
      if(item.status==="failed"&&!item.errors.length)item.errors.push(`Expected ${test.expectedStatus}; received ${result.status}`);
      const attachment=result.attachments.find(a=>a.name==="errspec-mutation");
      if(attachment){
        let data;
        try{data=JSON.parse(attachment.body?.toString()||readFileSync(attachment.path,"utf8"));}
        catch{this.protocolError="Invalid Playwright mutation fixture metadata";item.started=false;return;}
        if(data.runId!==this.state.context.runId||data.requestId!==this.state.context.requestId){this.protocolError="Stale errspec Playwright observation";item.started=false;return;}
        if(data.nodeObserverVersion!==1){this.errors.push("Playwright mutation fixture does not support Node observations; reinstall the matching @errspec/playwright artifact");item.started=false;return;}
        const object=value=>value!==null&&typeof value==="object"&&!Array.isArray(value);
        if(!object(data.hits)||!object(data.infected)||Object.values(data.hits).some(count=>!Number.isSafeInteger(count)||count<0)||Object.values(data.infected).some(value=>value!==true)){this.protocolError="Invalid Playwright mutation fixture facts";item.started=false;return;}
        item.hits=data.hits;item.infected=Object.keys(data.infected);
      }else if(item.started){item.started=false;this.errors.push(`Missing errspec Playwright fixture metadata: ${item.file}: ${item.name}. Import test from @errspec/playwright or extend mutationFixtures.`);}
    }
    onError(error){this.errors.push(error.stack||error.message||String(error));}
    onEnd(){writeFileSync(this.state.report,JSON.stringify({tests:[...this.tests.values()],errors:this.errors,...(this.protocolError?{protocolError:this.protocolError}:{}),...(this.acquisitionIncomplete?{acquisitionIncomplete:this.acquisitionIncomplete}:{})}));}
  };
}
export async function createPlaywrightAdapter(ctx){
  const {root,options,packageDir,remember,failure}=ctx;
  let inventory=ctx.inventory;
  const viteDir=resolvePackage(root,"vite");
  if(!viteDir)throw failure("EC_MUTATION_CONFIG","Playwright mutation requires a project Vite installation");
  const configFile=options.runnerConfig||["ts","mts","js","mjs","cts","cjs"].map(ext=>join(root,`playwright.config.${ext}`)).find(existsSync);
  if(!configFile)throw failure("EC_MUTATION_CONFIG","Cannot find playwright.config.*; use --runner-config");
  remember(configFile);remember(join(viteDir,"package.json"));
  const directory=join(options.temp,"playwright");mkdirSync(directory,{recursive:true});
  const wrapper=join(directory,"config.mjs"),reporterFile=join(directory,"reporter.cjs"),stateFile=join(directory,"state.json"),reportFile=join(directory,"report.json");
  writeFileSync(reporterFile,`module.exports=(${reporter.toString()})(require);`);
  const configRoot=dirname(configFile);
  const weakRuntime=readFileSync(join(options.toolTemp,"weak-runtime.mjs"),"utf8").replace("export function attachWeakBinary","function attachWeakBinary");
  // Preserve the user's fixture/project/config loading. Our Vite server chooses
  // a private port per owned worker; relative config paths keep their old base.
  writeFileSync(wrapper,`import original from ${JSON.stringify(pathToFileURL(configFile).href)};\nimport {resolve} from "node:path";\nconst base=${JSON.stringify(configRoot)};\nconst absolute=value=>value?resolve(base,value):undefined;\nconst servers=[original.webServer||[]].flat();\nif(servers.some(server=>server.command&&!/^(?:vite|vp dev)(?:\\s|$)/.test(server.command)))throw new Error("errspec Playwright supports a Vite app; custom webServer commands are unsupported");\nconst project=p=>({...p,retries:0,repeatEach:1,timeout:(p.timeout??original.timeout??30000)*Number(process.env.ERRSPEC_TIMEOUT_MULTIPLIER||1),testDir:absolute(p.testDir||original.testDir||"."),snapshotDir:absolute(p.snapshotDir||original.snapshotDir),use:{...original.use,...p.use,baseURL:process.env.ERRSPEC_VITE_URL}});\nexport default {...original,...project(original),globalSetup:[original.globalSetup||[]].flat().map(absolute),globalTeardown:[original.globalTeardown||[]].flat().map(absolute),webServer:undefined,workers:1,retries:0,repeatEach:1,fullyParallel:false,maxFailures:process.env.ERRSPEC_COMPLETE==="1"?0:original.maxFailures,updateSnapshots:"none",outputDir:${JSON.stringify(join(directory,"test-results"))},reporter:[[${JSON.stringify(reporterFile)}]],projects:original.projects?.map(project)};`);
  const vite=await import(pathToFileURL(join(viteDir,"dist/node/index.js")).href);
  let phase="plain";
  const plugin={name:"errspec-mutation",enforce:"pre",configResolved(config){
    if(config.configFile)remember(config.configFile);
    for(const file of config.configFileDependencies||[])remember(file);
  },transform(code,id){
    let file=id.split("?")[0];if(existsSync(file))file=realpathSync(file);
    const entry=inventory.get(file);if(!entry||phase==="plain")return;
    if(code!==entry.source)throw failure("EC_MUTATION_INSTRUMENTATION",`Source transformed before browser mutation instrumentation: ${ctx.rel(file)}`);
    return entry.emitted;
  }};
  let server;
  const cli=join(packageDir,"cli.js");
  return {async execute(nextPhase,request,entries,baseline){
    phase=nextPhase;inventory=entries;
    if(server){await server.close();server=undefined;}
    let url="http://127.0.0.1";
    if(nextPhase!=="inspect"){
      server=await vite.createServer({root,cacheDir:join(options.temp,"vite-cache"),plugins:[plugin],server:{host:"127.0.0.1",port:0,strictPort:true,open:false}});
      await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}`;
    }
    const context={runId:ctx.runId,requestId:request.requestId,activeKey:request.key||null,hitLimit:request.hitLimit||Number.MAX_SAFE_INTEGER,weakMutation:options.weakMutation};
    const selectedTaskIds=(request.selected||[]).map(id=>{const test=baseline.tests.find(t=>t.id===id);if(!test)throw failure("EC_MUTATION_PROTOCOL",`Unknown test ID: ${id}`);return test.taskId;});
    const nodeObservations=join(directory,`observations-${request.requestId}-${nextPhase}`);rmSync(nodeObservations,{recursive:true,force:true});mkdirSync(nodeObservations,{recursive:true});
    writeFileSync(stateFile,JSON.stringify({root,phase:nextPhase,nodeObservations,nodeObserver:pathToFileURL(join(options.toolTemp,"child-process.mjs")).href,report:reportFile,context,matrix:!!options.killMatrix,complete:!!request.complete,selectedTaskIds,
      runtime:`(()=>{${weakRuntime}\n(${browserRuntime.toString()})(${JSON.stringify(context)},attachWeakBinary);})();`}));
    rmSync(reportFile,{force:true});
    const args=[cli,"test","--config",wrapper,...(nextPhase==="inspect"?["--list"]:[]),...(options.runnerProject?["--project",options.runnerProject]:[])];
    try{
      const outcome=await runChild(args,{root,env:{ERRSPEC_BROWSER_STATE:stateFile,ERRSPEC_VITE_URL:url,ERRSPEC_TIMEOUT_MULTIPLIER:String(request.multiplier||1),ERRSPEC_COMPLETE:request.complete?"1":"0"},onProgress:()=>ctx.progress(request)});
      if(!existsSync(reportFile))throw failure("EC_MUTATION_CONFIG",`Playwright failed before providing test results (${outcome.code??outcome.signal}); inspect stderr`);
      const report=JSON.parse(readFileSync(reportFile,"utf8"));
      if(report.protocolError)throw failure("EC_MUTATION_PROTOCOL",report.protocolError);
      const files=[...new Set(report.tests.map(t=>resolve(root,t.file)))];files.forEach(file=>remember(file));
      const specs=report.tests.map(t=>({file:t.file,project:JSON.parse(t.id)[0],id:t.id}));
      if(nextPhase==="inspect"){
        if(outcome.code!==0||report.errors.length)throw failure("EC_MUTATION_CONFIG",report.errors.join("\n")||"Playwright test discovery failed");
        return {specs,excluded:[configFile,...files]};
      }
      const execution={...(report.acquisitionIncomplete?{acquisitionIncomplete:report.acquisitionIncomplete}:{}),selected:request.selected||[],tests:report.tests,failedModules:[],errors:report.errors,hits:{},staticHits:{},moduleDurations:{},wallTimeout:false};
      for(const test of report.tests){
        for(const [key,count]of Object.entries(test.hits))execution.hits[key]=(execution.hits[key]||0)+count;
        execution.moduleDurations[test.file]=(execution.moduleDurations[test.file]||0)+test.duration;
      }
      for(const file of readdirSync(nodeObservations)){
        const data=JSON.parse(readFileSync(join(nodeObservations,file),"utf8"));
        if(data.runId!==ctx.runId||data.requestId!==request.requestId||data.phase!==nextPhase)throw failure("EC_MUTATION_PROTOCOL","Stale Playwright Node observations");
        for(const fact of data.facts)ctx.recordChildProcesses(execution,ctx.rel(fact.file),[fact]);
      }
      if(![0,1].includes(outcome.code))execution.errors.push(`Playwright process terminated: ${outcome.code??outcome.signal}`);
      for(const environment of Object.values(server.environments))for(const module of environment.moduleGraph?.idToModuleMap?.values()||[]){
        if(module.file&&!module.file.includes("/node_modules/")&&existsSync(module.file))remember(module.file);
      }
      return {execution,specs,excluded:[configFile,...files]};
    }finally{if(server){await server.close();server=undefined;}}
  }};
}
