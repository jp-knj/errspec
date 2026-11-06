import { spawnSync } from "node:child_process";

const MAX_BUFFER = 64 * 1024 * 1024;
const MAX_INPUT = 1024 * 1024;
function failure(code,message) { return Object.assign(new Error(message),{code}); }
function invoke(binary,input,count=1) {
  const result=spawnSync(binary,["__mutation-source"],{
    input:JSON.stringify(input),encoding:"utf8",maxBuffer:MAX_BUFFER,timeout:60000*count,
  });
  if(result.error?.code==="ENOBUFS")return undefined;
  if(result.status!==0)throw failure(result.stderr?.startsWith("EC_MUTATION_INPUT:")?"EC_MUTATION_INPUT":"EC_MUTATION_INSTRUMENTATION",result.stderr||result.error?.message||"Native parser failed");
  try{return JSON.parse(result.stdout);}catch{throw failure("EC_MUTATION_PROTOCOL","Invalid native generator response");}
}

export function nativeSource(binary,engineFingerprint,request) {
  const result=invoke(binary,{...request,engineFingerprint});
  if(result===undefined)throw failure("EC_MUTATION_INSTRUMENTATION","Native parser response exceeded buffer limit");
  return result;
}

// tsrs arenas live until process exit. Bound both batch size and source bytes;
// a single large file retains the existing isolated invocation and buffer limit.
export function nativeSources(binary,engineFingerprint,requests) {
  const results=[];
  for(let start=0;start<requests.length;) {
    let end=start,bytes=0;
    while(end<requests.length&&end-start<8) {
      const size=Buffer.byteLength(JSON.stringify(requests[end]));
      if(end>start&&bytes+size>MAX_INPUT)break;
      bytes+=size;end++;
    }
    const group=requests.slice(start,end);
    let acquired;
    if(group.length>1)acquired=invoke(binary,{requests:group,engineFingerprint},group.length);
    // Aggregate output can exceed the limit even when each file fits. Retry
    // independently instead of turning a transport limit into a syntax failure.
    if(acquired===undefined)acquired=group.map(request=>{
      try{return {Ok:nativeSource(binary,engineFingerprint,request)};}
      catch(error){if(error.code==="EC_MUTATION_INPUT"||error.code==="EC_MUTATION_PROTOCOL")throw error;return {Err:error.message};}
    });
    if(!Array.isArray(acquired)||acquired.length!==group.length||acquired.some(value=>!value||typeof value!=="object"||Object.keys(value).length!==1||!(typeof value.Err==="string"||value.Ok&&Array.isArray(value.Ok.candidates))))
      throw failure("EC_MUTATION_PROTOCOL","Invalid native batch response");
    results.push(...acquired);start=end;
  }
  return results;
}
