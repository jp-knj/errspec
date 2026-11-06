import { writeFileSync } from "node:fs";
import { join } from "node:path";

/** Executed against the installed tarballs by all three Distribution platforms. */
export function installedPluginSmoke(directory, run, runnerPackage) {
  writeFileSync(join(directory,"src/mutation.ts"),"export function add(a:number,b:number){return a+b;}\n");
  writeFileSync(join(directory,"mutation.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/mutation";test("adds",()=>expect(add(2,3)).toBe(5));');
  writeFileSync(join(directory,"vitest.config.mjs"),'export default {test:{include:["mutation.test.ts"]}};');
  writeFileSync(join(directory,"plugin-dependency.mjs"),"export const replacement='a * b';");
  writeFileSync(join(directory,"mutation-plugin.mjs"),`
import {defineMutator,defineIgnorer,definePlugin} from '@errspec/core';
import {replacement} from './plugin-dependency.mjs';
export default definePlugin({name:'installed',cache:'deterministic',
  mutators:[defineMutator({name:'InstalledMath',visit(node,ctx){if(node.kind==='BinaryExpression'&&node.text==='a+b')return [{replacement:ctx.settings.override??replacement}]}})],
  ignorers:[defineIgnorer({name:'documented',shouldIgnore(node){if(node.kind==='StringLiteral')return 'documentation'}})]});
`);
  writeFileSync(join(directory,"plugin-api.mjs"),`
import assert from 'node:assert/strict';
import {runMutation} from '@errspec/core';
const warm=process.argv.includes('--warm'), changed=process.argv.includes('--changed');
const result=await runMutation({node:process.execPath,vitestPackage:${JSON.stringify(runnerPackage)},reuseSession:true,cache:true,cacheDir:'plugin-cache',plugins:[{module:'./mutation-plugin.mjs',settings:{override:null}}]});
assert.equal(result.exitCode,0,JSON.stringify(result));
const custom=result.report.mutants.filter(m=>m.mutator==='InstalledMath');
assert.equal(custom.length,1);assert.equal(custom[0].status,'Killed');assert.equal(result.report.cache.disabledReason,undefined);
if(warm){assert.equal(result.report.cache.executed,0);assert.ok(result.report.cache.reused>0);}
else{assert.equal(result.report.cache.reused,0);assert.ok(result.report.cache.executed>0);}
if(changed)assert.equal(result.report.cache.invalidated,true);
`);
  run(process.execPath,["plugin-api.mjs"]);
  run(process.execPath,["plugin-api.mjs","--warm"]);
  writeFileSync(join(directory,"plugin-dependency.mjs"),"export const replacement='a / b';");
  run(process.execPath,["plugin-api.mjs","--changed"]);
}
