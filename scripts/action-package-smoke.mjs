// Exercise the Action against the actual installed native CLI, without registry access.
import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {withInstalledCore} from './installed-core.mjs';
const action=new URL('./action.mjs',import.meta.url).href;
if(!process.env.ERRSPEC_VITEST_PACKAGE)throw new Error('Set ERRSPEC_VITEST_PACKAGE for installed Action acceptance');
withInstalledCore(process.argv[2]??`artifacts/npm/manifest-${process.platform}-${process.arch}.json`,({root,manifest,run})=>{
  mkdirSync(join(root,'src'));
  writeFileSync(join(root,'src/value.ts'),'export const enabled=true;\n');
  writeFileSync(join(root,'value.test.ts'),`import {test,expect} from ${JSON.stringify(pathToFileURL(join(resolve(process.env.ERRSPEC_VITEST_PACKAGE),'dist/index.js')).href)};import {enabled} from './src/value.ts';test('enabled',()=>expect(enabled).toBeDefined());`);
  writeFileSync(join(root,'vitest.config.mjs'),'export default {test:{include:["value.test.ts"]}};');
  writeFileSync(join(root,'consumer.mjs'),`
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {prepare,run} from ${JSON.stringify(action)};
process.env.GITHUB_WORKSPACE=process.cwd();
process.env.ERRSPEC_OPTIONS=JSON.stringify({vitestPackage:${JSON.stringify(resolve(process.env.ERRSPEC_VITEST_PACKAGE))},policy:{failOnSurvived:true}});
const setup=await prepare();
const state=JSON.parse(readFileSync(setup.state,'utf8'));
assert.match(state.binary,/node_modules[\\/]@errspec[\\/]cli-/,'resolve the installed native artifact');
assert.equal(await run(setup.state),1);
const report=JSON.parse(readFileSync(setup.reportDirectory+'/native.json','utf8'));
assert.equal(report.runComplete,true);assert.equal(report.gate.passed,false);assert.ok(report.summary.survived>0);
assert.ok(report.cache.saved>0);
assert.match(readFileSync(setup.reportDirectory+'/index.html','utf8'),/errspec mutation report/);
`);
  run(process.execPath,['consumer.mjs'],0,{GITHUB_STEP_SUMMARY:join(root,'summary'),GITHUB_OUTPUT:join(root,'outputs'),GITHUB_EVENT_PATH:''});
  console.log(JSON.stringify({target:manifest.target,checks:['installed-action-engine-discovery','installed-action-native-gate','installed-action-reports-and-history']},null,2));
});
