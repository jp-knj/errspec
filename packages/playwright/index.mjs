import {readFileSync} from "node:fs";
import {test as base} from "@playwright/test";
export {expect} from "@playwright/test";
// An automatic fixture also works with an application's existing extended test.
// There is no runtime work outside an errspec-owned execution.
export const mutationFixtures={
  _errspecMutation:[async({context},use,testInfo)=>{
    const path=process.env.ERRSPEC_BROWSER_STATE;
    if(!path){await use();return;}
    const state=JSON.parse(readFileSync(path,"utf8"));
    if(state.selectedTaskIds.length&&!state.selectedTaskIds.includes(testInfo.testId)){testInfo.skip();return;}
    const data={runId:state.context.runId,requestId:state.context.requestId,hits:{},infected:{}};
    await context.exposeBinding("__errspecMutationHit",(_source,event)=>{
      if(event.runId!==data.runId||event.requestId!==data.requestId)throw new Error("Stale errspec browser observation");
      if(event.infected)data.infected[event.key]=true;
      else data.hits[event.key]=(data.hits[event.key]||0)+1;
    });
    await context.addInitScript({content:state.runtime});
    try{await use();}
    finally{
      // Every hit is sent through a binding, so closed pages and earlier
      // documents retain their observations in this test's Node fixture.
      for(const page of context.pages())for(const frame of page.frames()){
        try{await frame.evaluate(async()=>{await Promise.all(globalThis.__errspecMutationPending||[]);});}catch(error){if(!page.isClosed()&&!frame.isDetached())throw error;}
      }
      await testInfo.attach("errspec-mutation",{body:Buffer.from(JSON.stringify(data)),contentType:"application/json"});
    }
  },{auto:true}],
};
export const test=base.extend(mutationFixtures);
