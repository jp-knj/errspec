import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync,cpSync,symlinkSync,realpathSync} from "node:fs";
import {resolve,join,dirname} from "node:path";
import {tmpdir} from "node:os";
import {fileURLToPath} from "node:url";
import {createHash} from "node:crypto";
const repo=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const manifestPath=resolve(process.argv[2]);
const manifest=JSON.parse(readFileSync(manifestPath,"utf8"));
const dependencies=realpathSync(process.env.ERRSPEC_RUNNER_DEPENDENCIES||join(repo,"tests/fixtures/runners/node_modules"));
const compilers=JSON.parse(process.env.ERRSPEC_TYPESCRIPT_PACKAGES||JSON.stringify([join(repo,"packages/analysis-driver/node_modules/typescript6"),join(repo,"packages/analysis-driver/node_modules/typescript7")]));
const directory=realpathSync(mkdtempSync(join(tmpdir(),"errspec-runner-packages-")));
let standalone;
function run(cwd,command,args){
  const output=spawnSync(command,args,{cwd,encoding:"utf8",timeout:120000,env:process.env});
  assert.equal(output.status,0,`${command} ${args.join(" ")}\n${output.error||""}\n${output.stderr}\n${output.stdout}`);return output.stdout;
}
function link(root,name,target=join(dependencies,name)){
  mkdirSync(dirname(join(root,"node_modules",name)),{recursive:true});symlinkSync(realpathSync(target),join(root,"node_modules",name),"dir");
}
try{
  writeFileSync(join(directory,"package.json"),JSON.stringify({private:true,type:"module"}));
  const tarballs=manifest.packages.map(pkg=>{
    const file=resolve(manifestPath,"..",pkg.filename);
    assert.equal(createHash("sha256").update(readFileSync(file)).digest("hex"),pkg.sha256);return file;
  });
  run(directory,"npm",["install","--offline","--ignore-scripts","--no-audit","--no-fund","--cache",join(directory,"cache"),...tarballs]);
  for(const runner of ["vitest","jest","playwright"]){
    const root=join(directory,runner);mkdirSync(root);
    cpSync(join(repo,"tests/fixtures/runners",runner==="vitest"?"vite-plus":runner),root,{recursive:true});
    if(runner==="vitest"){
      writeFileSync(join(root,"value.test.ts"),readFileSync(join(root,"value.test.ts"),"utf8").replace("vite-plus/test","vitest"));
      link(directory,"vitest");
    }else if(runner==="jest"){
      link(directory,"jest");link(directory,"@jest/globals");
    }else{
      link(directory,"@playwright/test");link(directory,"vite");
    }
    const api=runner==="playwright"?"@errspec/core":`@errspec/${runner}`;
    writeFileSync(join(root,".gitignore"),"node_modules/\n.errspec/\n");
    writeFileSync(join(root,"installed.mjs"),`import assert from "node:assert/strict";import {readFileSync,writeFileSync} from "node:fs";import {runMutation} from ${JSON.stringify(api)};import {selectTests} from "@errspec/core";const result=await runMutation({root:process.cwd(),node:process.execPath,${runner==="playwright"?'runner:"playwright",':''}weakMutation:true,killMatrix:true});assert.equal(result.exitCode,0,JSON.stringify(result));assert.equal(result.report.runComplete,true);assert.ok(result.report.summary.killed>0);assert.equal(result.report.summary.runtimeError,0);const covering=new Set(result.report.mutants.filter(m=>m.coveredBy.length).map(m=>m.key));assert.ok(covering.size>0);assert.equal(result.report.killMatrix.mutants.full,covering.size);for(const row of result.report.killMatrix.rows){assert.equal(row.complete,covering.has(row.key));if(row.complete)assert.equal(row.executed.length,row.selected.length);}writeFileSync("src/value.ts",readFileSync("src/value.ts","utf8").replace("=>","=>/* changed */"));const selection=await selectTests({root:process.cwd(),node:process.execPath,runner:${JSON.stringify(runner)},since:"HEAD"});assert.equal(selection.exitCode,0,JSON.stringify(selection));assert.equal(selection.report.runComplete,true);assert.deepEqual(selection.report.files,[${JSON.stringify(runner==="playwright"?"value.spec.mjs":"value.test.ts")}]);`);
    run(root,"git",["init","-q","-b","main"]);run(root,"git",["add","."]);
    run(root,"git",["-c","commit.gpgSign=false","-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-q","-m","fixture"]);
    run(root,process.execPath,["installed.mjs"]);
  }
  for(const compiler of compilers){
    const root=join(directory,"typecheck-"+compilers.indexOf(compiler));mkdirSync(root);
    const source="export const value:number=42;export const safe=(x:{n:number})=>x?.n;";writeFileSync(join(root,"value.ts"),source);
    writeFileSync(join(root,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,skipLibCheck:true,noUncheckedIndexedAccess:true},files:["value.ts"]}));
    writeFileSync(join(root,"installed.mjs"),`
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {checkMutants,checkMutantsInWorker} from "@errspec/typecheck";
const source=${JSON.stringify(source)},options={root:process.cwd(),typescriptPackage:${JSON.stringify(realpathSync(compiler))}};
const number=source.indexOf("42"),chain=source.indexOf("x?.n");
const mutants=[
  {key:"bad",file:"value.ts",byteOffset:number,end:number+2,original:"42",replacement:"'wrong'"},
  {key:"mislabeled",file:"value.ts",byteOffset:chain,end:chain+4,original:"x?.n",replacement:"x.missing",mutator:"OptionalChaining"},
  {key:"safe",file:"value.ts",byteOffset:chain,end:chain+4,original:"x?.n",replacement:"x.n",mutator:"OptionalChaining"},
];
const facts=await checkMutants(options,mutants),worker=await checkMutantsInWorker(options,mutants);
assert.deepEqual(worker,facts);
assert.match(facts.outcomes[0].diagnostic,/TS2322/);assert.equal(facts.outcomes[0].equivalentReason,null);
assert.match(facts.outcomes[1].diagnostic,/TS2339/);assert.equal(facts.outcomes[1].equivalentReason,null);
assert.equal(facts.outcomes[2].diagnostic,null);assert.match(facts.outcomes[2].equivalentReason,/equivalent:/);
assert.equal(readFileSync("value.ts","utf8"),source);
`);
    run(root,process.execPath,["installed.mjs"]);
  }
  link(directory,"@types");
  writeFileSync(join(directory,"strict.mts"),'import {runMutation as vitest} from "@errspec/vitest";import {runMutation as jest} from "@errspec/jest";import {checkMutants,checkMutantsInWorker,type TypecheckFacts} from "@errspec/typecheck";import {test,mutationFixtures} from "@errspec/playwright";void vitest({runner:"vitest"});void jest({runner:"jest"});const facts:Promise<TypecheckFacts>=checkMutants({},[{key:"x",file:"a.ts",byteOffset:0,end:1,original:"a",replacement:"b"}]);void facts;void checkMutantsInWorker({},[]);test("typed",async({page})=>{await page.goto("/")});void mutationFixtures;\n// @ts-expect-error runner package fixes its runner\nvoid vitest({runner:"jest"});');
  writeFileSync(join(directory,"strict.mjs"),`import assert from "node:assert/strict";import {createRequire} from "node:module";const ts=createRequire(import.meta.url)(${JSON.stringify(realpathSync(compilers[0]))});const program=ts.createProgram(["strict.mts"],{strict:true,noEmit:true,skipLibCheck:false,types:["node"],typeRoots:[${JSON.stringify(join(directory,"node_modules/@types"))}],lib:["lib.es2022.d.ts","lib.dom.d.ts","lib.esnext.disposable.d.ts"],target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.NodeNext,moduleResolution:ts.ModuleResolutionKind.NodeNext});assert.deepEqual(ts.getPreEmitDiagnostics(program).map(d=>ts.flattenDiagnosticMessageText(d.messageText,"\\n")),[]);`);
  run(directory,process.execPath,["strict.mjs"]);
  standalone=realpathSync(mkdtempSync(join(tmpdir(),"errspec-typecheck-independent-")));writeFileSync(join(standalone,"package.json"),JSON.stringify({private:true,type:"module"}));
  const typecheckTar=manifest.packages.find(p=>p.name==="@errspec/typecheck");assert.ok(typecheckTar);
  run(standalone,"npm",["install","--offline","--ignore-scripts","--no-audit","--no-fund","--cache",join(standalone,"cache"),resolve(manifestPath,"..",typecheckTar.filename)]);
  writeFileSync(join(standalone,"value.ts"),"export const value:number=42;");
  writeFileSync(join(standalone,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,skipLibCheck:true},files:["value.ts"]}));
  writeFileSync(join(standalone,"independent.mjs"),`import assert from "node:assert/strict";import {createRequire} from "node:module";import {checkMutants} from "@errspec/typecheck";assert.throws(()=>createRequire(import.meta.url).resolve("@errspec/cli"));const facts=await checkMutants({root:process.cwd(),typescriptPackage:${JSON.stringify(realpathSync(compilers[0]))}},[{key:"independent",file:"value.ts",byteOffset:26,end:28,original:"42",replacement:"'wrong'"}]);assert.match(facts.outcomes[0].diagnostic,/TS2322/);`);
  run(standalone,process.execPath,["independent.mjs"]);
  console.log(JSON.stringify({ok:true,checks:["installed-vitest-core-request","installed-jest-core-request","installed-playwright-fixture","installed-complete-matrix","installed-current-git-selection","installed-typescript6-and7-facts","worker-thread-parity","installed-strict-declarations","standalone-typecheck-without-cli"]}));
}finally{rmSync(directory,{recursive:true,force:true});if(standalone)rmSync(standalone,{recursive:true,force:true});}
