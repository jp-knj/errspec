// Node 24 publishes async spawn facts but no synchronous child_process diagnostics.
// Keep the async APIs untouched; restore the three synchronous exports after the test module.
import { subscribe, unsubscribe } from 'node:diagnostics_channel';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { fileURLToPath } from 'node:url';

export function observeChildProcesses(record) {
  const pending=new WeakMap(),patched=[];
  let active=true;
  const capture=(argv,cwd,method)=>{
    if(!active||!Array.isArray(argv)||!argv.every(v=>typeof v==='string'))return;
    record({argv:[...argv],cwd:cwd instanceof URL?fileURLToPath(cwd):cwd===undefined?process.cwd():String(cwd),method});
  };
  const start=({process:child,options})=>pending.set(child,{argv:options.args,cwd:options.cwd});
  const end=({process:child})=>{const fact=pending.get(child);pending.delete(child);if(fact)capture(fact.argv,fact.cwd,'spawn');};
  const error=({process:child})=>pending.delete(child);
  subscribe('tracing:child_process.spawn:start',start);
  subscribe('tracing:child_process.spawn:end',end);
  subscribe('tracing:child_process.spawn:error',error);
  for(const method of ['spawnSync','execFileSync','execSync']) {
    const descriptor=Object.getOwnPropertyDescriptor(childProcess,method),original=descriptor.value;
    function observed(...args) {
      let result,failure;
      try {result=Reflect.apply(original,this,args);}catch(error){failure=error;}
      // Failed validation/launches create no process; nonzero exits can still be observed by a passing test.
      const launched=method==='spawnSync'?result?.pid:!failure||failure.pid;
      if(!launched){if(failure)throw failure;return result;}
      let options,argv;
      if(method==='execSync') {options=args[1];argv=[options?.shell||'/bin/sh','-c',args[0]];}
      else {const list=Array.isArray(args[1])?args[1]:[];options=Array.isArray(args[1])?args[2]:args[1];argv=[args[0],...list];}
      capture(argv,options?.cwd,method);
      if(failure)throw failure;
      return result;
    }
    for(const property of ['name','length'])Object.defineProperty(observed,property,Object.getOwnPropertyDescriptor(original,property));
    Object.defineProperty(childProcess,method,{...descriptor,value:observed});
    patched.push({method,descriptor,observed});
  }
  syncBuiltinESMExports();
  return ()=>{
    if(!active)return;
    active=false;
    unsubscribe('tracing:child_process.spawn:start',start);
    unsubscribe('tracing:child_process.spawn:end',end);
    unsubscribe('tracing:child_process.spawn:error',error);
    for(const {method,descriptor,observed} of patched) {
      // Do not overwrite a later user-installed wrapper.
      if(childProcess[method]===observed)Object.defineProperty(childProcess,method,descriptor);
    }
    syncBuiltinESMExports();
  };
}
