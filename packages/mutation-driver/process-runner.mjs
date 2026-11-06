import { spawn } from "node:child_process";
// Children inherit the owned driver's process group. Rust terminates the complete
// group on deadlines and interruption; adapters never detach child processes.
export async function runChild(args,{root,env,onProgress,capture=false}) {
  return await new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,args,{cwd:root,env:{...process.env,...env},stdio:["ignore","pipe","pipe","ipc"]});
    let stdout="";
    child.stdout.on("data",data=>{if(capture)stdout+=data;else process.stderr.write(data);});
    child.stderr.on("data",data=>process.stderr.write(data));
    child.on("message",message=>{if(message?.errspec==="testStart")onProgress();});
    child.on("error",reject);
    child.on("close",(code,signal)=>resolve({code,signal,stdout}));
  });
}
