import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,symlinkSync,realpathSync,existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {runMutation} from '../packages/core/index.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=join(repo,'target/debug/errspec');
const source='export function add(a:number,b:number){return a+b;}\nexport function limit(n:number){return n>=2;}\nexport const ready=true;\n';
function fixture(t){
 const root=realpathSync(mkdtempSync(join(tmpdir(),'errspec-shard-')));t.after(()=>rmSync(root,{recursive:true,force:true}));
 mkdirSync(join(root,'src'));mkdirSync(join(root,'node_modules'));symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(root,'node_modules/vitest'),'dir');
 writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
 writeFileSync(join(root,'src/value.ts'),source);
 writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {add,limit,ready} from "./src/value";test("guards",()=>{expect(add(2,3)).toBe(5);expect(limit(2)).toBe(true);expect(limit(1)).toBe(false);expect(ready).toBe(true)});');
 writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');return root;
}
function execute(root,args=[]){return spawnSync(binary,['mutate','--node',process.execPath,'--format','json',...args],{cwd:root,encoding:'utf8',timeout:60000});}
function run(root,args=[],expected=0){const result=execute(root,args);assert.ifError(result.error);assert.equal(result.status,expected,result.stderr+result.stdout);return JSON.parse(result.stdout);}
const ids=report=>report.mutants.map(m=>m.key).sort();
const partition=(key,total)=>Number(BigInt('0x'+key.slice(0,16))%BigInt(total))+1;
test('shards partition the requested inventory without duplicates and preserve keys across positions',{timeout:60000},t=>{
 const root=fixture(t),full=run(root,['--time-budget','0']),combined=[];
 assert.equal(full.shard,undefined);assert.ok(full.summary.total>=5);
 for(let index=1;index<=3;index++){
  const part=run(root,['--shard',`${index}/3`,'--time-budget','0']);
  assert.equal(part.shard.eligible,full.summary.total);assert.equal(part.shard.selected,part.summary.total);
  assert.equal(part.shard.selected+part.shard.omitted,full.summary.total);assert.equal(part.shard.algorithm,'sha256_prefix_v1');
  assert.ok(part.mutants.every(m=>partition(m.key,3)===index));combined.push(...ids(part));
 }
 assert.equal(new Set(combined).size,combined.length);assert.deepEqual(combined.sort(),ids(full));
 const whole=run(root,['--shard','1/1','--time-budget','0']);assert.deepEqual(ids(whole),ids(full));
 writeFileSync(join(root,'src/value.ts'),'\n\n'+source);
 const moved=run(root,['--shard','2/3','--time-budget','0']);assert.deepEqual(ids(moved),ids(full).filter(key=>partition(key,3)===2));
});
test('configuration, CLI and API compose the same shard and null clears configured selection',{timeout:60000},async t=>{
 const root=fixture(t);writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({shard:{index:2,total:3},policy:{timeBudget:0}}));
 const configured=run(root),cli=run(root,['--shard','1/2']);assert.equal(configured.shard.index,2);assert.equal(cli.shard.index,1);assert.equal(cli.shard.total,2);
 const api=await runMutation({node:process.execPath,shard:{index:1,total:2}},{binary,cwd:root});assert.equal(api.exitCode,0);assert.deepEqual(ids(api.report),ids(cli));
 const cleared=await runMutation({node:process.execPath,shard:null},{binary,cwd:root});assert.equal(cleared.exitCode,0);assert.equal(cleared.report.shard,undefined);assert.equal(cleared.report.summary.total,configured.shard.eligible);
});
test('invalid shard syntax and schema fail before tests begin',{timeout:60000},t=>{
 const root=fixture(t),marker=join(root,'started');writeFileSync(join(root,'value.test.ts'),`import {test} from 'vitest';import {writeFileSync} from 'node:fs';test('started',()=>writeFileSync(${JSON.stringify(marker)},'yes'));`);
 for(const input of ['0/2','1/0','3/2','-1/2','1','1/2/3']){const result=execute(root,['--shard',input]);assert.equal(result.status,2);assert.equal(existsSync(marker),false);}
 for(const shard of [{index:0,total:2},{index:1,total:0},{index:3,total:2},{index:1.5,total:2},{index:1,total:2,extra:true}]){
  writeFileSync(join(root,'errspec.mutation.config.json'),JSON.stringify({shard}));const report=run(root,[],2);assert.equal(report.testsExecuted,false);assert.equal(report.diagnostics[0].code,'EC_MUTATION_CONFIG');assert.equal(existsSync(marker),false);
 }
});
test('shards intersect Git and only filters while pending selected candidates cannot pass gates',{timeout:60000},t=>{
 const root=fixture(t),git=(...args)=>{const result=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(result.status,0,result.stderr);};
 writeFileSync(join(root,'.gitignore'),'node_modules/\n');git('init','--quiet');git('add','.');git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','-c','commit.gpgSign=false','commit','--quiet','-m','base');
 writeFileSync(join(root,'src/value.ts'),source+'export const next=true;\n');
 const diff=run(root,['--since','HEAD','--time-budget','0']),parts=[];
 for(let index=1;index<=2;index++){const part=run(root,['--since','HEAD','--shard',`${index}/2`,'--time-budget','0']);assert.equal(part.shard.eligible,diff.summary.total);assert.equal(part.selection.selected,diff.summary.total);parts.push(...ids(part));}
 assert.deepEqual(parts.sort(),ids(diff));
 const key=diff.mutants.find(m=>m.status==='Pending').key,index=partition(key,2);
 const only=run(root,['--only',key,'--shard',`${index}/2`,'--time-budget','0','--fail-on-undetected'],1);
 assert.deepEqual(ids(only),[key]);assert.equal(only.summary.score,null);assert.equal(only.gate.passed,false);assert.equal(only.shard.eligible,1);
 const omitted=run(root,['--only',key,'--shard',`${index===1?2:1}/2`,'--time-budget','0']);assert.equal(omitted.summary.total,0);assert.equal(omitted.shard.omitted,1);
});
test('sequential shards share full probe coverage and accumulate results for an unsharded repeat',{timeout:60000},t=>{
 const root=fixture(t),first=run(root,['--cache','--reuse-session','--shard','1/2']),second=run(root,['--cache','--reuse-session','--shard','2/2']);
 assert.equal(second.cache.coverageReused,true);assert.equal(second.cache.invalidated,false);
 const warm=run(root,['--cache','--reuse-session']);assert.equal(warm.cache.coverageReused,true);assert.equal(warm.cache.executed,0);
 const parts=[...first.mutants,...second.mutants].map(m=>[m.key,m.status]).sort();assert.deepEqual(warm.mutants.map(m=>[m.key,m.status]).sort(),parts);
 assert.equal(warm.cache.reused,warm.mutants.filter(m=>['Killed','Survived'].includes(m.status)&&m.attempts.length===1).length);
 assert.equal(readFileSync(join(root,'src/value.ts'),'utf8'),source);
});
test('typechecking and per-line sampling operate within the reported shard scope',{timeout:60000},t=>{
 const root=fixture(t);writeFileSync(join(root,'src/value.ts'),source.replace('b:number){','b:number):number{').replace('n:number){','n:number):boolean{'));
 writeFileSync(join(root,'tsconfig.json'),JSON.stringify({compilerOptions:{strict:true,target:'ES2022',module:'ESNext',moduleResolution:'Bundler',skipLibCheck:true},files:['src/value.ts']}));
 const full=run(root,['--time-budget','0']),block=full.mutants.find(m=>m.mutator==='BlockStatement'),index=partition(block.key,2);
 const typed=run(root,['--shard',`${index}/2`,'--typecheck','--typecheck-package',join(repo,'packages/analysis-driver/node_modules/typescript6'),'--time-budget','0']);
 assert.equal(typed.typecheck.checked,typed.mutants.filter(m=>m.ignored===null).length);assert.ok(typed.mutants.some(m=>m.key===block.key&&m.status==='CompileError'));
 assert.equal(typed.shard.eligible,full.summary.total);assert.equal(typed.shard.selected,typed.summary.total);
 const sampled=run(root,['--shard',`${index}/2`,'--mutants-per-line','1','--time-budget','0']);assert.equal(sampled.shard.selected,sampled.summary.total);assert.equal(sampled.summary.score,null);
 for(const line of new Set(sampled.mutants.map(m=>m.line)))assert.ok(sampled.mutants.filter(m=>m.line===line&&m.reason?.startsWith('Time budget')).length<=1);
});
