// Native init-skill and the installed skills-npm layout must contain identical bytes.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {join} from 'node:path';
import {withInstalledCore} from './installed-core.mjs';
function files(root){return readdirSync(root,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(join(root,entry.name)).map(file=>join(entry.name,file)):[entry.name]).sort();}
withInstalledCore(process.argv[2]??`artifacts/npm/manifest-${process.platform}-${process.arch}.json`,({root,manifest,run})=>{
  const launcher=join(root,'node_modules/@errspec/cli/bin/errspec.mjs'),source=join(root,'node_modules/@errspec/cli/skills'),target=join(root,'.agents/skills');
  assert.deepEqual(readdirSync(source).sort(),['errspec','errspec-playwright','errspec-test-loop']);
  const expected=new Map(files(source).map(file=>[file,readFileSync(join(source,file),'utf8')]));
  const init=(args=[],code=0)=>JSON.parse(run(process.execPath,[launcher,'init-skill','--format','json',...args],code));
  assert.ok(init().skills.every(skill=>skill.status==='written'));
  for(const file of files(source))assert.equal(readFileSync(join(source,file),'utf8'),readFileSync(join(target,file),'utf8'),file);
  assert.ok(init().skills.every(skill=>skill.status==='unchanged'));
  writeFileSync(join(target,'errspec/SKILL.md'),'reviewed project customization');
  rmSync(join(target,'errspec-playwright'),{recursive:true});
  assert.equal(init([],2).ok,false);assert.equal(existsSync(join(target,'errspec-playwright/SKILL.md')),false);
  assert.equal(readFileSync(join(target,'errspec/SKILL.md'),'utf8'),'reviewed project customization');
  assert.equal(init(['--force']).ok,true);
  for(const file of files(source))assert.equal(readFileSync(join(source,file),'utf8'),readFileSync(join(target,file),'utf8'),file);
  rmSync(source,{recursive:true});
  const native=join(root,`node_modules/@errspec/cli-${manifest.target}/bin/errspec`);
  assert.ok(JSON.parse(run(native,['init-skill','--dir','standalone','--format','json'],0,{PATH:''})).skills.every(skill=>skill.status==='written'));
  for(const [file,content] of expected)assert.equal(readFileSync(join(root,'standalone',file),'utf8'),content);
  console.log(JSON.stringify({target:manifest.target,checks:['installed-skills-npm-layout','embedded-init-skill-bytes','idempotent-init','preflight-conflicts','explicit-force','standalone-no-source-or-node']},null,2));
});
