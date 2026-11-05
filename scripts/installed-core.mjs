// Offline consumer installation shared by installed core/Action/skill acceptance.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {targets} from '../packages/cli/platforms.mjs';
const repo=fileURLToPath(new URL('../',import.meta.url));
export function withInstalledCore(manifestPath, check) {
  manifestPath=resolve(manifestPath);
  const manifest=JSON.parse(readFileSync(manifestPath,'utf8'));
  assert.equal(manifest.target,`${process.platform}-${process.arch}`,'Test the current platform tarball');
  const root=mkdtempSync(join(tmpdir(),'errspec-installed-consumer-'));
  function run(command,args,code=0,env={}){const r=spawnSync(command,args,{cwd:root,env:{...process.env,...env},encoding:'utf8',timeout:120000});assert.equal(r.status,code,r.stdout+r.stderr);return r.stdout;}
  try {
  const dependencies={},packages={};
  for(const pkg of manifest.packages.filter(pkg=>['@errspec/core','@errspec/cli',`@errspec/cli-${manifest.target}`].includes(pkg.name))) {
    const path=resolve(manifestPath,'..',pkg.filename),bytes=readFileSync(path);
    assert.equal(createHash('sha256').update(bytes).digest('hex'),pkg.sha256);
    dependencies[pkg.name]=`file:${path}`;
    const metadata=pkg.name.startsWith('@errspec/cli-')?{os:[process.platform],cpu:[process.arch]}:JSON.parse(readFileSync(join(repo,'packages',pkg.name.split('/')[1],'package.json'),'utf8'));
    packages[`node_modules/${pkg.name}`]={version:pkg.version,resolved:`file:${path}`,integrity:`sha512-${createHash('sha512').update(bytes).digest('base64')}`,license:'MIT',...Object.fromEntries(['dependencies','optionalDependencies','engines','bin','os','cpu'].filter(k=>metadata[k]).map(k=>[k,metadata[k]]))};
  }
  // npm ci validates every optional dependency even when its OS/CPU excludes it.
  for(const [target,info] of Object.entries(targets)) {
    const name=`@errspec/cli-${target}`;
    packages[`node_modules/${name}`]??={version:manifest.version,optional:true,os:[info.os],cpu:[info.cpu],resolved:`file:unavailable-${target}.tgz`};
  }
  const project={name:'errspec-report-consumer',private:true,type:'module',dependencies};
  packages['']=project;
  writeFileSync(join(root,'package.json'),JSON.stringify(project));
  writeFileSync(join(root,'package-lock.json'),JSON.stringify({name:project.name,lockfileVersion:3,requires:true,packages}));
  run('npm',['ci','--offline','--ignore-scripts','--no-audit','--no-fund','--cache',join(root,'cache')]);
    return check({root,manifest,run});
  } finally {rmSync(root,{recursive:true,force:true});}
}
