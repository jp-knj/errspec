import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { resolve, join } from "node:path";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const manifestPath = resolve(
  process.argv[2] ||
    `artifacts/npm/manifest-${process.platform}-${process.arch}.json`,
);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const files = manifest.packages.map((pkg) => {
  const path = resolve(manifestPath, "..", pkg.filename);
  assert.equal(
    createHash("sha256").update(readFileSync(path)).digest("hex"),
    pkg.sha256,
  );
  return path;
});
const dir = mkdtempSync(join(tmpdir(), "errspec-consumer-"));
function run(command, args, expected = 0, environment={}) {
  const result = spawnSync(command, args, {
    cwd: dir,
    encoding: "utf8",
    timeout: 120000,
    env:{...process.env,...environment},
  });
  assert.equal(
    result.status,
    expected,
    `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}\n${result.error || ""}`,
  );
  return result.stdout;
}
try {
  writeFileSync(
    join(dir, "package.json"),
    JSON.stringify({
      name: "errspec-package-smoke",
      private: true,
      type: "module",
    }),
  );
  // A fresh cache and --offline prove that these tarballs contain the runnable CLI.
  run("npm", [
    "install",
    "--offline",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "--cache",
    join(dir, "cache"),
    ...files,
  ]);
  const launcher = join(dir, "node_modules/@errspec/cli/bin/errspec.mjs");
  assert.match(
    run(process.execPath, [launcher, "--version"]),
    new RegExp(`errspec ${manifest.version}`),
  );
  writeFileSync(
    join(dir, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: { strict: true, skipLibCheck: true },
      files: ["errors.ts"],
    }),
  );
  writeFileSync(
    join(dir, "errors.ts"),
    "export type Failure = { _tag: 'Offline' };\n",
  );
  const scan = JSON.parse(
    run(process.execPath, [launcher, "scan", "--format", "json"]),
  );
  assert.equal(scan.scanComplete, true);
  assert.equal(scan.candidates.length, 1);
  const init = JSON.parse(
    run(process.execPath, [
      launcher,
      "init",
      "--source",
      "errors.ts",
      "--export",
      "Failure",
      "--runner",
      "bun",
      "--format",
      "json",
    ]),
  );
  assert.equal(init.ready, false);
  const fail = JSON.parse(
    run(
      process.execPath,
      [
        launcher,
        "verify",
        "--runner",
        "bun",
        "--only-contract-tests",
        "--format",
        "json",
      ],
      1,
    ),
  );
  assert.equal(fail.testsExecuted, true);
  assert.equal(fail.ok, false);
  writeFileSync(
    join(dir, "errspec.generated.test.ts"),
    `import { test } from 'bun:test';
import { scenarioTest } from '@errspec/testkit';
scenarioTest(test, import.meta.url, {contract:'Failure',id:'case-1',tag:'Offline'},async s=>{
  const read=s.fault('dependency',async()=>({ok:false,error:{_tag:'Offline'}}));
  const result=await read(); s.check('behavior', result.ok, false);
});\n`,
  );
  // An unrelated failing test must be excluded only when the explicit option is used.
  writeFileSync(
    join(dir, "unrelated.test.ts"),
    "import { test } from 'bun:test'; test('unrelated',()=>{throw new Error('unrelated')});\n",
  );
  const pass = JSON.parse(
    run(process.execPath, [
      launcher,
      "verify",
      "--runner",
      "bun",
      "--only-contract-tests",
      "--format",
      "json",
    ]),
  );
  assert.equal(pass.ok, true);
  const all = JSON.parse(
    run(
      process.execPath,
      [launcher, "verify", "--runner", "bun", "--format", "json"],
      1,
    ),
  );
  assert.equal(all.ok, false);
  writeFileSync(join(dir,"api.mjs"), `
import assert from 'node:assert/strict';
import {defineConfig,runMutation} from '@errspec/core';
const result=await runMutation(defineConfig({include:['does-not-exist/**']}));
assert.equal(result.exitCode,2);
assert.equal(result.report.testsExecuted,false);
assert.equal(result.report.kind,'errspec/mutation');
assert.equal(result.report.diagnostics[0].code,'EC_MUTATION_CONFIG');
`);
  run(process.execPath,["api.mjs"]);
  const extraChecks=["installed-core-native-api"];
  for (const compiler of JSON.parse(process.env.ERRSPEC_TYPESCRIPT_PACKAGES || "[]")) {
    writeFileSync(join(dir,"tsconfig.json"),JSON.stringify({compilerOptions:{strict:true,target:"ES2022",module:"ESNext",moduleResolution:"Bundler",skipLibCheck:true},files:["errors.ts"]}));
    const options=["--analysis-engine","typescript","--typescript-package",resolve(compiler),"--analysis-node",process.execPath,"--format","json"];
    const scanned=JSON.parse(run(process.execPath,[launcher,"scan",...options]));
    assert.equal(scanned.scanComplete,true);assert.deepEqual(scanned.candidates[0].tags,["Offline"]);
    const checked=JSON.parse(run(process.execPath,[launcher,"check",...options]));
    assert.equal(checked.ok,true);assert.equal(checked.analysis.engine,"typescript");
    extraChecks.push(`embedded-analysis-driver-${checked.analysis.version}`);
  }
  if(process.env.ERRSPEC_VITEST_PACKAGE) {
    mkdirSync(join(dir,"src"));
    writeFileSync(join(dir,"src/mutation.ts"),"export function add(a:number,b:number){return a+b;}\n");
    writeFileSync(join(dir,"mutation.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/mutation";test("adds",()=>expect(add(2,3)).toBe(5));');
    writeFileSync(join(dir,"vitest.config.mjs"),'export default {test:{include:["mutation.test.ts"]}};');
    const mutation=JSON.parse(run(process.execPath,[launcher,"mutate","--node",process.execPath,"--vitest-package",resolve(process.env.ERRSPEC_VITEST_PACKAGE),"--fail-on-undetected","--format","json"]));
    assert.equal(mutation.runComplete,true);assert.equal(mutation.summary.killed,2);
    writeFileSync(join(dir,"api.mjs"), `
import assert from 'node:assert/strict';
import {defineConfig,runMutation} from '@errspec/core';
const diff=process.argv.includes('--diff');
const sharded=process.argv.includes('--shard');
const parallel=process.argv.includes('--parallel');
const cache=process.argv.includes('--cache'),warm=process.argv.includes('--cached');
const result=await runMutation(defineConfig({node:process.execPath,vitestPackage:${JSON.stringify(resolve(process.env.ERRSPEC_VITEST_PACKAGE))},reuseSession:true,cache,concurrency:parallel?2:1,...(diff?{since:'HEAD',policy:{timeBudget:0}}:sharded?{shard:{index:1,total:2},policy:{timeBudget:0}}:{failOnUndetected:true})}));
assert.equal(result.exitCode,0);
assert.equal(result.report.runComplete,true);
assert.equal(result.report.engine.sessionReuse,'available');
if(parallel)assert.deepEqual(result.report.engine.parallel,{requested:2,workers:warm?0:2});
if(sharded){assert.equal(result.report.shard.index,1);assert.equal(result.report.shard.total,2);assert.equal(result.report.shard.selected,result.report.summary.total);assert.equal(result.report.shard.selected+result.report.shard.omitted,result.report.shard.eligible);assert.ok(result.report.mutants.every(m=>BigInt('0x'+m.key.slice(0,16))%2n===0n));}
else if(diff){assert.equal(result.report.selection.selected,1);assert.ok(result.report.selection.omitted>0);assert.ok(result.report.mutants.every(m=>m.line===2));}
else assert.equal(result.report.summary.killed,2);
if(cache){assert.equal(result.report.cache.disabledReason,undefined);assert.equal(result.report.cache.reused,warm?2:0);assert.equal(result.report.cache.executed,warm?0:2);assert.equal(result.report.cache.saved,2);assert.equal(result.report.cache.coverageReused,warm?true:undefined);}
`);
    run(process.execPath,["api.mjs"]);
    run(process.execPath,["api.mjs","--cache"]);
    run(process.execPath,["api.mjs","--cache","--cached"]);
    run(process.execPath,["api.mjs","--shard"]);
    run(process.execPath,["api.mjs","--parallel"]);
    run(process.execPath,["api.mjs","--parallel","--cache","--cached"]);
    extraChecks.push("embedded-mutation-driver","installed-core-mutation","installed-core-result-cache","installed-core-shard-selection","installed-core-parallel-execution");
    writeFileSync(join(dir,".gitignore"),"node_modules/\ncache/\n.errspec/mutation-cache/\n");
    run("git",["init","--quiet"]);
    run("git",["add","."]);
    run("git",["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","-c","commit.gpgSign=false","commit","--quiet","-m","package baseline"]);
    writeFileSync(join(dir,"src/mutation.ts"),"export function add(a:number,b:number){return a+b;}\nexport const extra=true;\n");
    run(process.execPath,["api.mjs","--diff"]);
    extraChecks.push("installed-core-git-selection");
    const installed=JSON.parse(run(process.execPath,["--input-type=module","-e","import {nativeBinary} from '@errspec/cli/native';console.log(JSON.stringify(nativeBinary()));"]));
    run(process.execPath,["--test",fileURLToPath(new URL("../tests/mutation-ownership.test.mjs",import.meta.url))],0,{ERRSPEC_BIN:installed});
    extraChecks.push("installed-native-ownership-and-orphan-recovery");
  }
  console.log(
    JSON.stringify({
      target: manifest.target,
      version: manifest.version,
      checks: [
        "offline-install",
        "version",
        "scan",
        "init",
        "todo-fails",
        "completed-passes",
        "selected-files",
        "full-suite-fails",
        ...extraChecks,
      ],
    }),
  );
} finally {
  rmSync(dir, { recursive: true, force: true });
}
