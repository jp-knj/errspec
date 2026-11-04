import { spawn } from "node:child_process";
import { createRequire } from "node:module";

export function defineConfig(config) { return config; }

/** Run native planning/judgment directly; quality failure still returns its report. */
export async function runMutation(options = {}, transport = {}) {
  if (Number(process.versions.node.split(".")[0]) < 24) throw new Error("runMutation requires Node.js 24 or later");
  transport.signal?.throwIfAborted();
  const input = JSON.stringify({ protocol: 1, options, ...(transport.configFile ? { configFile: transport.configFile } : {}) }, (_key, value) => {
    if (["function", "symbol", "bigint"].includes(typeof value) || typeof value === "number" && !Number.isFinite(value)) throw new TypeError("Mutation options must contain JSON values only");
    return value;
  });
  const binary = transport.binary ?? (await import(createRequire(import.meta.url).resolve("@errspec/cli/native"))).nativeBinary();
  return new Promise((resolve,reject) => {
    const child=spawn(binary,["__mutation-api"],{stdio:["pipe","pipe","inherit"],cwd:transport.cwd});
    let output="", failure, killTimer;
    const interrupt=()=>{
      child.kill("SIGINT");
      killTimer??=setTimeout(()=>child.kill("SIGKILL"),5000).unref();
    };
    transport.signal?.addEventListener("abort",interrupt,{once:true});
    if(transport.signal?.aborted)interrupt();
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", chunk=>{
      if(failure)return;
      output+=chunk;
      if(Buffer.byteLength(output)>64*1024*1024){failure=new Error("Mutation API output exceeds 64 MiB");output="";interrupt();}
    });
    child.stdin.on("error",error=>{if(error.code!=="EPIPE")failure=error;});
    child.on("error",error=>{failure=error;});
    child.once("close",(code,signal)=>{
      transport.signal?.removeEventListener("abort",interrupt);
      clearTimeout(killTimer);
      try {
        if(failure)throw failure;
        if(signal)throw new Error(`Mutation API terminated by ${signal}`);
        const result=JSON.parse(output);
        if(result.protocol!==1 || result.exitCode!==code || result.report?.kind!=="errspec/mutation" || result.report?.schemaVersion!==1)throw new Error("Invalid mutation API response");
        resolve({exitCode:result.exitCode,report:result.report});
      }catch(error){reject(error);}
    });
    child.stdin.end(input);
  });
}

/** Render a native report through the shared Rust formatter. No tests are executed. */
export async function formatReport(report, options = {}, transport = {}) {
  if (Number(process.versions.node.split('.')[0]) < 24) throw new Error('formatReport requires Node.js 24 or later');
  const reporter = options.reporter ?? 'text';
  const binary = transport.binary ?? (await import(createRequire(import.meta.url).resolve('@errspec/cli/native'))).nativeBinary();
  const input = JSON.stringify({protocol:1,report,reporter,root:options.root??'.',sources:options.sources??{}}, (_key,value)=>{
    if (['function','symbol','bigint'].includes(typeof value) || typeof value==='number'&&!Number.isFinite(value)) throw new TypeError('Report options must contain JSON values only');
    return value;
  });
  if (Buffer.byteLength(input)>64*1024*1024) throw new Error('Report request exceeds 64 MiB');
  return new Promise((resolve,reject)=>{
    const child=spawn(binary,['__format-report'],{stdio:['pipe','pipe','inherit'],cwd:transport.cwd});
    let output='',failure;
    child.stdout.setEncoding('utf8');
    child.stdout.on('data',chunk=>{if(failure)return;output+=chunk;if(Buffer.byteLength(output)>128*1024*1024){failure=new Error('Formatter output exceeds 128 MiB');output='';child.kill('SIGKILL');}});
    child.stdin.on('error',error=>{if(error.code!=='EPIPE')failure=error;});
    child.on('error',error=>{failure=error;});
    child.once('close',(code,signal)=>{
      try {
        if(failure)throw failure;
        if(signal)throw new Error(`Formatter terminated by ${signal}`);
        const result=JSON.parse(output);
        if(result.protocol!==1)throw new Error('Invalid formatter response');
        if(code!==0)throw new Error(result.error??'Report formatting failed');
        if(typeof result.output!=='string')throw new Error('Invalid formatter response');
        resolve(result.output);
      }catch(error){reject(error);}
    });
    child.stdin.end(input);
  });
}
