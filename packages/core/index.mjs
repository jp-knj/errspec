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
