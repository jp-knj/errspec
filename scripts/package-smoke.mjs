import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { resolve, join } from "node:path";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";

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
function run(command, args, expected = 0) {
  const result = spawnSync(command, args, {
    cwd: dir,
    encoding: "utf8",
    timeout: 120000,
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
  const extraChecks=[];
  if(process.env.ERRSPEC_VITEST_PACKAGE) {
    mkdirSync(join(dir,"src"));
    writeFileSync(join(dir,"src/mutation.ts"),"export function add(a:number,b:number){return a+b;}");
    writeFileSync(join(dir,"mutation.test.ts"),'import {test,expect} from "vitest";import {add} from "./src/mutation";test("adds",()=>expect(add(2,3)).toBe(5));');
    writeFileSync(join(dir,"vitest.config.mjs"),'export default {test:{include:["mutation.test.ts"]}};');
    const mutation=JSON.parse(run(process.execPath,[launcher,"mutate","--node",process.execPath,"--vitest-package",resolve(process.env.ERRSPEC_VITEST_PACKAGE),"--fail-on-undetected","--format","json"]));
    assert.equal(mutation.runComplete,true);assert.equal(mutation.summary.killed,2);
    extraChecks.push("embedded-mutation-driver");
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
