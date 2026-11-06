import {readFileSync,realpathSync} from "node:fs";
import {resolve} from "node:path";
import {Worker} from "node:worker_threads";
import {applyCandidate,typecheckInventory} from "./compiler.mjs";
/** Source-bound compiler observations. No source files are written and no test verdict is inferred. */
export async function checkMutants(options,mutants,progress=()=>{}){
  for(const field of ["root","project","typescriptPackage"])if(options[field]!==undefined&&(typeof options[field]!=="string"||!options[field]))throw new TypeError(`${field} must be a nonempty path`);
  if(!Array.isArray(mutants)||typeof progress!=="function")throw new TypeError("Mutants must be an array and progress must be a function");
  const root=realpathSync(options.root||"."),inventory=new Map(),keys=new Set();
  for(const mutant of mutants){
    if(!mutant||typeof mutant.file!=="string"||!mutant.file||typeof mutant.original!=="string"||typeof mutant.replacement!=="string"||!Number.isSafeInteger(mutant.byteOffset)||!Number.isSafeInteger(mutant.end))throw new TypeError("Mutants require a source file, original/replacement text and integer UTF-8 ranges");
    if(typeof mutant.key!=="string"||!mutant.key||keys.has(mutant.key))throw new TypeError("Mutant keys must be nonempty and unique");
    keys.add(mutant.key);
    const file=realpathSync(resolve(root,mutant.file));
    if(!/\.[cm]?tsx?$/.test(file))throw new TypeError(`Typecheck source must be TypeScript: ${file}`);
    let entry=inventory.get(file);
    if(!entry){entry={source:readFileSync(file,"utf8"),candidates:[]};inventory.set(file,entry);}
    applyCandidate(entry.source,mutant);
    entry.candidates.push({...mutant,mutator:mutant.mutator||"Custom",ignored:null});
  }
  const facts=await typecheckInventory({root,typecheckProject:options.project?resolve(root,options.project):undefined,typecheckPackage:options.typescriptPackage?resolve(root,options.typescriptPackage):undefined},inventory,progress);
  for(const [file,entry]of inventory)if(readFileSync(file,"utf8")!==entry.source)throw new Error(`Typecheck input changed during acquisition: ${file}`);
  return facts;
}
/** Keep compiler acquisition off the calling thread, including TypeScript's synchronous APIs. */
export async function checkMutantsInWorker(options,mutants,{signal}={}){
  signal?.throwIfAborted();
  // --input-type belongs to the caller's stdin/eval source, not our worker file.
  // Retain the remaining flags, including import/loader configuration.
  const execArgv=[];
  for(let index=0;index<process.execArgv.length;index++){
    const argument=process.execArgv[index];
    if(argument==="--input-type")index++;
    else if(!argument.startsWith("--input-type="))execArgv.push(argument);
  }
  return new Promise((resolve,reject)=>{
    // Without an override Node retains its normal inherited-flag handling.
    const worker=new Worker(new URL("./worker.mjs",import.meta.url),{workerData:{options,mutants},execArgv:execArgv.length!==process.execArgv.length?execArgv:undefined});
    let received=false;
    const abort=()=>{void worker.terminate().then(()=>reject(signal.reason??new DOMException("Aborted","AbortError")),reject);};
    signal?.addEventListener("abort",abort,{once:true});
    if(signal?.aborted)abort();
    worker.once("message",message=>{received=true;if(message.error)reject(new Error(message.error));else resolve(message.result);});
    worker.once("error",reject);
    worker.once("exit",code=>{signal?.removeEventListener("abort",abort);if(!received&&!signal?.aborted)reject(new Error(`Typecheck worker exited without facts (${code})`));});
  });
}
