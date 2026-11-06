// Public synthetic fixture; no application source or credentials required.
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, symlinkSync, realpathSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import assert from "node:assert/strict";
import { compareReports } from "./compare-mutation-reports.mjs";

const {values}=parseArgs({options:{before:{type:"string"},binary:{type:"string"},reference:{type:"string"},dependencies:{type:"string"},output:{type:"string"},repeats:{type:"string",default:"3"}}});
if(!values.before||!values.output)throw new Error("Usage: node scripts/benchmark-mutation.mjs --before OLD_BINARY --output DIRECTORY [--binary BINARY] [--reference MUTATOR_CHECKOUT] [--dependencies NODE_MODULES] [--repeats 3]");
const repeats=Number(values.repeats);
if(!Number.isSafeInteger(repeats)||repeats<1)throw new Error("repeats must be a positive integer");
const output=resolve(values.output);mkdirSync(output,{recursive:true});
// Avoid overwriting an earlier observation or silently warming its snapshot.
assert.equal(readdirSync(output).length,0,"Output directory must be empty");
const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-benchmark-")));
const binaries={before:resolve(values.before),after:resolve(values.binary||"target/release/errspec")};
const timings=[],reports={};
function save(item,report){
  timings.push(item);reports[item.tool]=report;
  writeFileSync(join(output,`${item.tool}-${item.cold?"cold":"warm"}-${item.repeat}.json`),JSON.stringify({elapsedMs:item.elapsedMs,report},null,2)+"\n");
  writeFileSync(join(output,"timings.json"),JSON.stringify(timings,null,2)+"\n");
  console.log(JSON.stringify(item));
}
function native(tool,cold,repeat){
  const cache=join(root,".errspec","benchmark-"+tool);
  if(cold)rmSync(cache,{recursive:true,force:true});
  const start=performance.now();
  const result=spawnSync(binaries[tool],["mutate","--root",root,"--node",process.execPath,"--format","json","--cache","--cache-scope","--cache-dir",cache,"--reuse-session","--concurrency","4"],{encoding:"utf8",timeout:120000,maxBuffer:64*1024*1024});
  assert.ifError(result.error);assert.equal(result.status,0,result.stderr+result.stdout);
  const report=JSON.parse(result.stdout);assert.equal(report.runComplete,true);
  assert.equal(report.mutants.length,60);assert(report.mutants.every(m=>m.status==="Killed"));
  assert.equal(report.baseline.length,12);assert.equal(report.testsExecuted,true);
  assert.equal(report.cache.executed,cold?60:0);assert.equal(report.cache.reused,cold?0:60);
  save({tool,cold,repeat,elapsedMs:performance.now()-start,executed:report.cache.executed,reused:report.cache.reused,hostExit:result.status},report);
}
function reference(cold,repeat){
  const snapshot=join(root,".errspec","reference.json"),reportPath=join(output,"reference-raw.json");
  if(cold)rmSync(snapshot,{force:true});
  rmSync(reportPath,{force:true});
  const entry=pathToFileURL(resolve(values.reference,"packages/cli/src/index.ts")).href;
  const source=`import {runMutation} from ${JSON.stringify(entry)};import {writeFileSync} from 'node:fs';const start=performance.now();const report=await runMutation(${JSON.stringify({root,runner:"vitest",snapshotPath:snapshot,typecheck:false,weakMutation:false,arid:false,concurrency:4})});writeFileSync(${JSON.stringify(reportPath)},JSON.stringify({elapsedMs:performance.now()-start,report}));`;
  const start=performance.now();
  const result=spawnSync(process.execPath,["--input-type=module","-e",source],{cwd:resolve(values.reference),encoding:"utf8",timeout:120000,maxBuffer:64*1024*1024});
  assert.ifError(result.error);assert([0,1].includes(result.status),result.stderr);
  // Vitest may leave exitCode=1 after expected mutant assertion failures. Keep
  // that status visible and validate the complete controlled-fixture inventory.
  const value=JSON.parse(readFileSync(reportPath,"utf8"));
  assert.equal(value.report.entries.length,60);assert(value.report.entries.every(e=>e.status==="Killed"));
  assert.equal(value.report.executed,cold?60:0);
  save({tool:"mutator",cold,repeat,elapsedMs:performance.now()-start,apiElapsedMs:value.elapsedMs,executed:value.report.executed,dryRunFiles:value.report.dryRunFiles.length,hostExit:result.status},value.report);
  rmSync(reportPath);
}
try {
  mkdirSync(join(root,"src"));
  symlinkSync(resolve(values.dependencies||"examples/checkout/node_modules"),join(root,"node_modules"),"dir");
  writeFileSync(join(root,"package.json"),JSON.stringify({private:true,type:"module",dependencies:{vitest:"5.0.5"}}));
  writeFileSync(join(root,"vitest.config.mjs"),'export default {test:{include:["*.test.ts"]}};\n');
  for(let index=0;index<12;index++) {
    writeFileSync(join(root,`src/value-${index}.ts`),`export function value(n:number) { return n >= ${index+1}; }\n`);
    writeFileSync(join(root,`value-${index}.test.ts`),`import {test,expect} from "vitest";import {value} from "./src/value-${index}.ts";test("boundary",()=>{expect(value(${index+1})).toBe(true);expect(value(${index})).toBe(false)});\n`);
  }
  for(let repeat=0;repeat<repeats;repeat++) {
    for(const tool of repeat%2?["after","before"]:["before","after"]){native(tool,true,repeat);native(tool,false,repeat);}
    if(values.reference){reference(true,repeat);reference(false,repeat);}
    assert.deepEqual(reports.after.mutants.map(m=>[m.key,m.status,m.coveredBy,m.killedBy]),reports.before.mutants.map(m=>[m.key,m.status,m.coveredBy,m.killedBy]));
    if(values.reference) {
      const comparison=compareReports({native:reports.after,reference:reports.mutator,root,readSource:file=>readFileSync(join(root,file),"utf8")});
      assert.equal(comparison.differences.length,0);
      writeFileSync(join(output,"comparison.json"),JSON.stringify(comparison,null,2)+"\n");
    }
  }
} finally {rmSync(root,{recursive:true,force:true});}
