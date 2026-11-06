// Reproducible parser/validation throughput, separate from runner wall time.
import { parseArgs } from "node:util";
import { resolve } from "node:path";
import { nativeSource, nativeSources } from "../packages/mutation-driver/native-source.mjs";
import { instrument } from "../packages/mutation-driver/emitter.mjs";
import assert from "node:assert/strict";

const {values}=parseArgs({options:{binary:{type:"string"},before:{type:"string"},files:{type:"string",default:"200"},repeats:{type:"string",default:"5"}}});
const count=Number(values.files),repeats=Number(values.repeats);
if(!Number.isSafeInteger(count)||count<1||!Number.isSafeInteger(repeats)||repeats<1)throw new Error("files and repeats must be positive integers");
const binary=resolve(values.binary||"target/release/errspec"),before=resolve(values.before||binary);
const requests=Array.from({length:count},(_,index)=>({file:`src/value-${index}.ts`,source:`// 日本語 🧪\nexport function value(n:number){return n>=${index+1};}\n`,facts:true}));
function run(executable,batched) {
  const start=performance.now();
  const generated=batched?nativeSources(executable,undefined,requests):requests.map(request=>({Ok:nativeSource(executable,undefined,request)}));
  const validations=generated.map((result,index)=>{
    assert(result.Ok,result.Err);return {file:requests[index].file,source:instrument(requests[index].source,result.Ok.candidates,requests[index].file,"benchmark").code,validateOnly:true};
  });
  const checked=batched?nativeSources(executable,undefined,validations):validations.map(request=>({Ok:nativeSource(executable,undefined,request)}));
  for(const result of checked)assert(result.Ok,result.Err);
  return {elapsedMs:performance.now()-start,generated,checked};
}
const timings={individual:[],batched:[]};
for(let repeat=0;repeat<repeats;repeat++) {
  let individual,batched;
  // Alternate order to reduce a consistent filesystem/thermal advantage.
  if(repeat%2){batched=run(binary,true);individual=run(before,false);}
  else {individual=run(before,false);batched=run(binary,true);}
  assert.deepEqual(batched.generated,individual.generated);
  assert.deepEqual(batched.checked,individual.checked);
  timings.individual.push(individual.elapsedMs);timings.batched.push(batched.elapsedMs);
}
const median=values=>{const sorted=values.toSorted((a,b)=>a-b);return (sorted[Math.floor((sorted.length-1)/2)]+sorted[Math.ceil((sorted.length-1)/2)])/2;};
const individual=median(timings.individual),batched=median(timings.batched);
console.log(JSON.stringify({kind:"errspec/source-throughput",files:count,repeats,node:process.versions.node,platform:process.platform,architecture:process.arch,timingsMs:timings,medianMs:{individual,batched},speedup:individual/batched,exactResultsEqual:true},null,2));
