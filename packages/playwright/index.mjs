import {readFileSync,writeFileSync} from "node:fs";
import {join} from "node:path";
import {test as base} from "@playwright/test";
export {expect} from "@playwright/test";
let activeTest,lastModule;
// The worker fixture covers user fixture setup/test/teardown. Test identity is
// assigned only while its mutation fixture is active; hooks stay module-scoped.
export const mutationFixtures={
  _errspecNodeProcessObserver:[async({},use)=>{
    const path=process.env.ERRSPEC_BROWSER_STATE;
    if(!path){await use();return;}
    const state=JSON.parse(readFileSync(path,"utf8"));
    const {observeChildProcesses}=await import(state.nodeObserver),facts=[];
    const stop=observeChildProcesses(fact=>{
      let info;try{info=base.info();}catch{}
      const file=info?.file||lastModule;if(!file)return;
      facts.push({...fact,file,taskId:activeTest===info?.testId?activeTest:null});
    });
    try{await use();}
    finally{
      stop();
      writeFileSync(join(state.nodeObservations,`${process.pid}.json`),JSON.stringify({runId:state.context.runId,requestId:state.context.requestId,phase:state.phase,facts}));
    }
  },{auto:true,scope:"worker"}],
  _errspecMutation:[async({context,_errspecNodeProcessObserver},use,testInfo)=>{
    const path=process.env.ERRSPEC_BROWSER_STATE;
    if(!path){await use();return;}
    const state=JSON.parse(readFileSync(path,"utf8"));
    if(state.selectedTaskIds.length&&!state.selectedTaskIds.includes(testInfo.testId)){testInfo.skip();return;}
    lastModule=testInfo.file;activeTest=testInfo.testId;
    const data={nodeObserverVersion:1,runId:state.context.runId,requestId:state.context.requestId,hits:{},infected:{}};
    await context.exposeBinding("__errspecMutationHit",(_source,event)=>{
      if(event.runId!==data.runId||event.requestId!==data.requestId)throw new Error("Stale errspec browser observation");
      if(event.infected)data.infected[event.key]=true;
      else data.hits[event.key]=(data.hits[event.key]||0)+1;
    });
    await context.addInitScript({content:state.runtime});
    try{await use();}
    finally{
      try{
        for(const page of context.pages())for(const frame of page.frames()){
          try{await frame.evaluate(async()=>{await Promise.all(globalThis.__errspecMutationPending||[]);});}catch(error){if(!page.isClosed()&&!frame.isDetached())throw error;}
        }
        await testInfo.attach("errspec-mutation",{body:Buffer.from(JSON.stringify(data)),contentType:"application/json"});
      }finally{activeTest=undefined;}
    }
  },{auto:true}],
};
export const test=base.extend(mutationFixtures);
