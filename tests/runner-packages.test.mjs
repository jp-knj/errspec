import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,realpathSync,rmSync,cpSync,symlinkSync} from "node:fs";
import {join,resolve,dirname} from "node:path";
import {tmpdir} from "node:os";
import {pathToFileURL,fileURLToPath} from "node:url";
import {createRequire} from "node:module";
import {checkMutants,checkMutantsInWorker} from "../packages/typecheck/index.mjs";
const repo=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const binary=process.env.ERRSPEC_BIN||join(repo,"target/debug/errspec");
const dependencies=join(repo,"tests/fixtures/runners/node_modules");
function temporary(t){const root=realpathSync(mkdtempSync(join(tmpdir(),"errspec-public-package-")));t.after(()=>rmSync(root,{recursive:true,force:true}));return root;}
function compilerProject(t){
  const root=temporary(t),source='export const value:number=42;export const safe=(x:{n:number})=>x?.n;';
  writeFileSync(join(root,"value.ts"),source);
  writeFileSync(join(root,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,noUncheckedIndexedAccess:true,skipLibCheck:true},files:["value.ts"]}));
  const at=source.indexOf("42"),chain=source.indexOf("x?.n");
  return {root,source,mutants:[{key:"bad",file:"value.ts",byteOffset:at,end:at+2,original:"42",replacement:"'bad'"},{key:"safe",file:"value.ts",mutator:"OptionalChaining",byteOffset:chain,end:chain+4,original:"x?.n",replacement:"x.n"}]};
}
for(const major of [5,6,7])test(`public TS${major} facts and worker acquisition share the actual compiler implementation`,async t=>{
  const {root,source,mutants}=compilerProject(t),options={root,typescriptPackage:realpathSync(join(repo,`packages/analysis-driver/node_modules/typescript${major}`))};
  const facts=await checkMutants(options,mutants),worker=await checkMutantsInWorker(options,mutants);
  assert.deepEqual(worker,facts);assert.equal(facts.info.checked,2);assert.equal(facts.info.compileErrors,1);assert.equal(facts.info.equivalent,1);
  assert.match(facts.outcomes[0].diagnostic,/TS2322/);assert.equal(facts.outcomes[0].equivalentReason,null);
  assert.equal(facts.outcomes[1].diagnostic,null);assert.match(facts.outcomes[1].equivalentReason,/equivalent:/);
  assert.ok(facts.outcomes.every(fact=>!Object.hasOwn(fact,"status")),"compiler acquisition supplies facts, not mutation verdicts");
  assert.equal(readFileSync(join(root,"value.ts"),"utf8"),source);
});
test("public compiler validates source identity and cancels worker acquisition",async t=>{
  const {root,source,mutants}=compilerProject(t),options={root,typescriptPackage:realpathSync(join(repo,"packages/analysis-driver/node_modules/typescript6"))};
  await assert.rejects(checkMutants(options,[{...mutants[0],original:"stale"}]),/Invalid original/);
  await assert.rejects(checkMutants(options,[mutants[0],mutants[0]]),/unique/);
  await assert.rejects(checkMutants({...options,project:""},mutants),/nonempty path/);
  await assert.rejects(checkMutants(options,[{...mutants[0],byteOffset:0.5}]),/integer UTF-8/);
  const controller=new AbortController();controller.abort(new Error("cancelled before compiler spawn"));
  await assert.rejects(checkMutantsInWorker(options,mutants,{signal:controller.signal}),/cancelled before/);
  const active=new AbortController(),pending=checkMutantsInWorker(options,mutants,{signal:active.signal});
  setTimeout(()=>active.abort(new Error("cancelled active worker")),10);
  await assert.rejects(pending,/cancelled active worker/);
  await assert.rejects(checkMutants(options,[mutants[0]],()=>writeFileSync(join(root,"value.ts"),source.replace("42","41"))),/input changed during acquisition/);
  writeFileSync(join(root,"value.ts"),source);
});
function copyPackages(root){
  for(const name of ["core","vitest","jest","typecheck","playwright"]){const target=join(root,"node_modules/@errspec",name);mkdirSync(dirname(target),{recursive:true});cpSync(join(repo,"packages",name),target,{recursive:true});}
}
for(const runner of ["vitest","jest"])test(`public ${runner} entry delegates the same native core request with a fixed runner`,async t=>{
  const root=temporary(t);copyPackages(root);
  cpSync(join(repo,"tests/fixtures/runners",runner==="vitest"?"vite-plus":runner),root,{recursive:true});
  if(runner==="vitest")writeFileSync(join(root,"value.test.ts"),readFileSync(join(root,"value.test.ts"),"utf8").replace("vite-plus/test","vitest"));
  const link=name=>{mkdirSync(dirname(join(root,"node_modules",name)),{recursive:true});symlinkSync(realpathSync(join(dependencies,name)),join(root,"node_modules",name),"dir");};
  if(runner==="vitest")link("vitest");else{link("jest");link("@jest/globals");}
  const api=await import(pathToFileURL(join(root,"node_modules/@errspec",runner,"index.mjs")).href);
  const core=await import(pathToFileURL(join(root,"node_modules/@errspec/core/index.mjs")).href);
  const options={root,node:process.execPath,weakMutation:true},transport={binary,cwd:root};
  const result=await api.runMutation(options,transport),shared=await core.runMutation({...options,runner},transport);
  assert.equal(result.exitCode,0);assert.deepEqual(result.report.mutants.map(m=>[m.key,m.status,m.coveredBy]),shared.report.mutants.map(m=>[m.key,m.status,m.coveredBy]));
  assert.ok(result.report.summary.killed>0);assert.equal(result.report.summary.runtimeError,0);
  await assert.rejects(api.runMutation({...options,runner:"playwright"},transport),/fixes runner/);
});
test("public package declarations compose under strict TypeScript settings",t=>{
  const root=temporary(t);copyPackages(root);
  symlinkSync(realpathSync(join(dependencies,"@playwright")),join(root,"node_modules/@playwright"),"dir");
  symlinkSync(realpathSync(join(dependencies,"@types")),join(root,"node_modules/@types"),"dir");
  writeFileSync(join(root,"package.json"),JSON.stringify({type:"module"}));
  const file=join(root,"consumer.mts");
  writeFileSync(file,'import {runMutation as vitest} from "@errspec/vitest";import {runMutation as jest} from "@errspec/jest";import {checkMutants,checkMutantsInWorker,type TypecheckFacts} from "@errspec/typecheck";import {test,mutationFixtures} from "@errspec/playwright";void vitest({root:".",runner:"vitest"});void jest({runner:"jest"});const checked:Promise<TypecheckFacts>=checkMutants({root:"."},[{key:"x",file:"a.ts",byteOffset:0,end:1,original:"a",replacement:"b"}]);void checked;void checkMutantsInWorker({},[]);test("typed",async({page})=>{await page.goto("/")});void mutationFixtures;\n// @ts-expect-error runner package fixes its own runner\nvoid vitest({runner:"jest"});\n// @ts-expect-error compiler facts require byte offsets\nvoid checkMutants({},[{key:"x",file:"a.ts",original:"a",replacement:"b"}]);');
  const ts=createRequire(import.meta.url)(join(repo,"packages/analysis-driver/node_modules/typescript6"));
  const program=ts.createProgram([file],{strict:true,noEmit:true,skipLibCheck:false,types:["node"],typeRoots:[join(root,"node_modules/@types")],lib:["lib.es2022.d.ts","lib.dom.d.ts","lib.esnext.disposable.d.ts"],target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.NodeNext,moduleResolution:ts.ModuleResolutionKind.NodeNext});
  const diagnostics=ts.getPreEmitDiagnostics(program);
  assert.deepEqual(diagnostics.map(d=>ts.flattenDiagnosticMessageText(d.messageText,"\n")),[]);
});
