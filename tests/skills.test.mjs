import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,readdirSync,rmSync,symlinkSync,realpathSync,existsSync,statSync,chmodSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
const require=createRequire(new URL('../packages/report-tests/package.json',import.meta.url)),{parse}=require('yaml');
const repo=fileURLToPath(new URL('../',import.meta.url)),binary=join(repo,'target/debug/errspec'),shipped=join(repo,'packages/cli/skills');
function files(root){return readdirSync(root,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(join(root,entry.name)).map(file=>join(entry.name,file)):[entry.name]).sort();}
function project(t){const root=realpathSync(mkdtempSync(join(tmpdir(),'errspec-skills-')));t.after(()=>rmSync(root,{recursive:true,force:true}));return root;}
function cli(root,args,code=0){const r=spawnSync(binary,args,{cwd:root,encoding:'utf8',timeout:60000});assert.ifError(r.error);assert.equal(r.status,code,r.stdout+r.stderr);return r;}
function initialize(root,args=[],code=0){return JSON.parse(cli(root,['init-skill','--format','json',...args],code).stdout);}
function verifyCopy(target){for(const file of files(shipped))assert.equal(readFileSync(join(target,file),'utf8'),readFileSync(join(shipped,file),'utf8'),file);}
test('shipped skills are discoverable, parse as Agent Skills, and retain local references',()=>{
  const names=readdirSync(shipped).sort();assert.deepEqual(names,['errspec','errspec-playwright','errspec-test-loop']);
  assert.ok(JSON.parse(readFileSync(join(repo,'packages/cli/package.json'),'utf8')).files.includes('skills'));
  for(const name of names){
    const root=join(shipped,name),text=readFileSync(join(root,'SKILL.md'),'utf8'),frontmatter=parse(text.match(/^---\n([\s\S]*?)\n---/)[1]);
    assert.equal(frontmatter.name,name);assert.match(name,/^[a-z0-9]+(?:-[a-z0-9]+)*$/);assert.ok(name.length<=64);
    assert.equal(typeof frontmatter.description,'string');assert.ok(frontmatter.description.length>0&&frontmatter.description.length<=1024);
    assert.equal(frontmatter.license,'MIT');
    for(const file of files(root).filter(file=>file.endsWith('.md'))){
      const body=readFileSync(join(root,file),'utf8');
      for(const [,link] of body.matchAll(/\]\(([^)#\s]+)\)/g)){
        if(/^[a-z]+:/.test(link))continue;
        const target=resolve(dirname(join(root,file)),link);assert.ok(target.startsWith(root+'/'));assert.ok(existsSync(target),link);
      }
    }
  }
});
test('init-skill copies every packaged file, is idempotent, and preserves edited or unrelated content',t=>{
  const root=project(t),target=join(root,'.agents/skills');
  const first=initialize(root);assert.equal(first.kind,'errspec/skills-init');assert.ok(first.skills.every(s=>s.status==='written'));verifyCopy(target);
  const originalTime=statSync(join(target,'errspec/SKILL.md')).mtimeMs;
  assert.ok(initialize(root).skills.every(s=>s.status==='unchanged'));assert.equal(statSync(join(target,'errspec/SKILL.md')).mtimeMs,originalTime);
  writeFileSync(join(target,'errspec/references/configuration.md'),'my maintained settings\n');
  writeFileSync(join(target,'errspec/personal.md'),'keep this');
  rmSync(join(target,'errspec-playwright'),{recursive:true});
  const refused=initialize(root,[],2);assert.equal(refused.ok,false);assert.match(refused.diagnostics[0].message,/differs/);
  assert.equal(existsSync(join(target,'errspec-playwright/SKILL.md')),false,'preflight before writing another missing skill');
  assert.equal(readFileSync(join(target,'errspec/references/configuration.md'),'utf8'),'my maintained settings\n');
  const forced=initialize(root,['--force']);assert.equal(forced.ok,true);verifyCopy(target);
  assert.equal(readFileSync(join(target,'errspec/personal.md'),'utf8'),'keep this');
  const custom=join(root,'.claude/skills');initialize(root,['--dir',custom]);verifyCopy(custom);
});
test('symlinked skill targets/directories are rejected even with force',t=>{
  const root=project(t),outside=project(t),target=join(root,'skills');
  mkdirSync(join(target,'errspec'),{recursive:true});writeFileSync(join(outside,'SKILL.md'),'private content');
  symlinkSync(join(outside,'SKILL.md'),join(target,'errspec/SKILL.md'));
  assert.equal(initialize(root,['--dir','skills','--force'],2).ok,false);
  assert.equal(readFileSync(join(outside,'SKILL.md'),'utf8'),'private content');
  rmSync(join(target,'errspec/SKILL.md'));symlinkSync(outside,join(target,'errspec/references'));
  assert.equal(initialize(root,['--dir','skills','--force'],2).ok,false);
  assert.equal(existsSync(join(target,'errspec-playwright/SKILL.md')),false);
  symlinkSync(outside,join(root,'.agents'));
  assert.equal(initialize(root,['--force'],2).ok,false);
  assert.equal(existsSync(join(outside,'skills')),false);
});
test('a later write failure restores earlier overwritten bytes and removes newly written files',{skip:process.getuid?.()===0?'permission failure is not enforceable as root':false},t=>{
  const root=project(t),target=join(root,'skills');
  mkdirSync(join(target,'errspec'),{recursive:true});writeFileSync(join(target,'errspec/SKILL.md'),'before explicit force');chmodSync(join(target,'errspec/SKILL.md'),0o640);
  mkdirSync(join(target,'errspec-test-loop'));chmodSync(join(target,'errspec-test-loop'),0o555);
  try {assert.equal(initialize(root,['--dir','skills','--force'],2).ok,false);}
  finally {chmodSync(join(target,'errspec-test-loop'),0o755);}
  assert.equal(readFileSync(join(target,'errspec/SKILL.md'),'utf8'),'before explicit force');
  assert.equal(statSync(join(target,'errspec/SKILL.md')).mode&0o777,0o640);
  assert.equal(existsSync(join(target,'errspec/references/runners.md')),false);
  assert.equal(existsSync(join(target,'errspec-playwright/SKILL.md')),false);
});
test('documented mutation loop closes a real gap and verifies the selected ID before the final inventory',t=>{
  const root=project(t);initialize(root);
  mkdirSync(join(root,'src'));mkdirSync(join(root,'node_modules'));
  symlinkSync(join(repo,'examples/checkout/node_modules/vitest'),join(root,'node_modules/vitest'));
  writeFileSync(join(root,'package.json'),JSON.stringify({type:'module',dependencies:{vitest:'5.0.5'}}));
  writeFileSync(join(root,'src/value.ts'),'export const enabled=true;\n');
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value";test("enabled",()=>expect(enabled).toBeDefined());');
  writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');
  const first=JSON.parse(cli(root,['mutate','--node',process.execPath,'--reporter','agent']).stdout);
  const key=first.gaps.find(g=>g.gap==='notDetected').key;
  writeFileSync(join(root,'value.test.ts'),'import {test,expect} from "vitest";import {enabled} from "./src/value";test("enabled",()=>expect(enabled).toBe(true));');
  const selected=JSON.parse(cli(root,['mutate','--node',process.execPath,'--only',key,'--format','json']).stdout);
  assert.equal(selected.mutants.length,1);assert.equal(selected.mutants[0].key,key);assert.equal(selected.mutants[0].status,'Killed');
  const full=JSON.parse(cli(root,['mutate','--node',process.execPath,'--fail-on-survived','--format','json']).stdout);
  assert.equal(full.runComplete,true);assert.equal(full.gate.passed,true);assert.equal(full.summary.pending,0);
});
test('documented runner and mutation flags exist in the corresponding current CLI',()=>{
  const runnerBinary=process.env.ERRSPEC_RUNNER_BINARY||binary;
  const help=[['--help'],['mutate','--help'],['accept','--help']].map(args=>{const r=spawnSync(runnerBinary,args,{encoding:'utf8'});assert.equal(r.status,0,r.stderr);return r.stdout;}).join('\n')+cli(repo,['mutate','--help']).stdout+cli(repo,['init-skill','--help']).stdout;
  const known=new Set([...help.matchAll(/--([a-z][a-z-]*)/g)].map(match=>match[1]));
  for(const file of files(shipped).filter(file=>file.endsWith('.md'))) {
    const text=readFileSync(join(shipped,file),'utf8');
    for(const [,flag] of text.matchAll(/--([a-z][a-z-]*)/g))assert.ok(known.has(flag),`${file}: --${flag}`);
  }
});
