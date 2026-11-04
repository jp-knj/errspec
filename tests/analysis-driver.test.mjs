import assert from "node:assert/strict";
import { test } from "node:test";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
  symlinkSync,
  realpathSync,
  existsSync,
  readdirSync,
  chmodSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join, dirname, delimiter } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(repo, process.env.ERRSPEC_BIN ?? "target/debug/errspec");
const compilers = [6, 7].map((major) => ({
  major,
  path: realpathSync(
    resolve(repo, `packages/analysis-driver/node_modules/typescript${major}`),
  ),
}));
const fixture = (name) =>
  readFileSync(resolve(repo, "tests/fixtures/analysis", name), "utf8");
const modern = {
  strict: true,
  target: "ES2022",
  module: "ESNext",
  moduleResolution: "Bundler",
  skipLibCheck: true,
};

function project(t, files, options = modern) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "errspec-analysis-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    writeFileSync(join(root, name), text);
  }
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: options,
      files: Object.keys(files).filter((f) => /\.[cm]?ts$/.test(f)),
    }),
  );
  return root;
}
function run(root, args, compiler, extra = []) {
  const flags = compiler
    ? [
        "--analysis-engine",
        "typescript",
        "--typescript-package",
        compiler.path,
        "--analysis-node",
        process.execPath,
      ]
    : [];
  const result = spawnSync(
    binary,
    [...args, ...flags, ...extra, "--format", "json"],
    {
      cwd: root,
      encoding: "utf8",
      timeout: 60000,
      env: {
        ...process.env,
        PATH: dirname(process.execPath) + delimiter + process.env.PATH,
      },
    },
  );
  assert.ifError(result.error);
  assert.equal(result.signal, null, result.stderr);
  return { code: result.status, report: JSON.parse(result.stdout) };
}
function semantic(result) {
  const { analysis, ...report } = result.report;
  if (report.engine?.name === "typescript")
    report.engine = {
      name: "tsrs",
      revision: "1bfd1e65be95131f9242a03687d37006214a96f6",
    };
  return { ...result, report };
}
function config(root, tags = ["Offline"]) {
  const cases = tags.map((tag) => ({
    tag,
    expect: "reject",
    test: { file: "case.test.ts", name: tag },
  }));
  writeFileSync(
    join(root, "errspec.json"),
    JSON.stringify({
      schemaVersion: 1,
      project: "tsconfig.json",
      contracts: [
        {
          id: "c",
          source: {
            file: "errors.ts",
            export: "Failure",
            discriminant: "_tag",
          },
          cases,
        },
      ],
    }),
  );
}

for (const compiler of compilers) {
  test(`TS ${compiler.major}: entry returns and escaping failures retain native judgments`, (t) => {
    for (const body of [
      `export function submit(n:number):Failure {
  const nested=()=>{return {_tag:'Denied' as const}};
  void nested;
  if(n<0) throw new RangeError('bad');
  if(n===0) return {_tag:'Offline'};
  return {_tag:'Denied'};
}`,
      `export const submit=async(n:number):Promise<Failure>=>{
  if(n<0) return Promise.reject(new RangeError('bad'));
  try { if(n===0) throw new Error('caught'); } catch {}
  return n===0 ? {_tag:'Offline'} : {_tag:'Denied'};
};`,
      `export const submit=(n:number):Failure=>n===0?{_tag:'Offline'}:{_tag:'Denied'};`,
    ]) {
      const root = project(t, {
        "errors.ts": `// 日本🧪\r\nexport type Failure={_tag:'Offline'}|{_tag:'Denied'};\n${body}\n`,
        "barrel.ts": "export {submit as send} from './errors';",
        "case.test.ts": "export {};",
      });
      config(root, ["Denied", "Offline"]);
      const path = join(root,"errspec.json"), cfg = JSON.parse(readFileSync(path,"utf8"));
      cfg.schemaVersion=2;
      cfg.contracts[0].cases=cfg.contracts[0].cases.map(c=>({id:c.tag,tag:c.tag,expect:c.expect,test:{file:"case.test.ts"},observe:{faults:[],assertions:["tag"]}}));
      cfg.contracts[0].entry = {file:"barrel.ts",export:"send"};
      writeFileSync(path,JSON.stringify(cfg));
      const native=run(root,["check"]), official=run(root,["check"],compiler);
      assert.equal(official.report.analysisComplete,true,JSON.stringify(official));
      assert.deepEqual(semantic(official),native);
      assert.ok(official.report.contracts[0].paths.length);
      cfg.contracts[0].entry.export="missing";
      writeFileSync(path,JSON.stringify(cfg));
      const missing=run(root,["check"],compiler);
      assert.equal(missing.code,2);
      assert.equal(missing.report.analysisComplete,false);
      assert.deepEqual(semantic(missing),run(root,["check"]));
    }
  });

  test(`TS ${compiler.major}: Bun reachability checks use official entry locations`, (t) => {
    const root=project(t,{"errors.ts":`export type Failure={_tag:'Offline'}|{_tag:'Denied'};
export function submit(n:number):Failure {
  if(n<0) return {_tag:'Denied'};
  if(n===0) return {_tag:'Offline'};
  return {_tag:'Offline'};
}`});
    const sdk=pathToFileURL(resolve(repo,"packages/testkit/index.mjs")).href;
    const cases=["Denied","Offline"].map(tag=>({id:tag,tag,expect:"failure",test:{file:"case.test.mjs"},observe:{faults:[],assertions:["tag"]}}));
    writeFileSync(join(root,"errspec.json"),JSON.stringify({schemaVersion:2,project:"tsconfig.json",contracts:[{id:"c",source:{file:"errors.ts",export:"Failure",discriminant:"_tag"},entry:{file:"errors.ts",export:"submit"},cases}]}));
    const source=`import {test} from 'bun:test';
import {scenarioTest} from ${JSON.stringify(sdk)};
import {submit} from './errors.ts';
for(const [tag,n] of [['Denied',-1],['Offline',0]]) scenarioTest(test,import.meta.url,{contract:'c',id:tag,tag,withoutFaults:true},async s=>{
s.check('tag',submit(n)._tag,tag);
// extra reach
});`;
    writeFileSync(join(root,"case.test.mjs"),source);
    const incomplete=run(root,["verify","--runner","bun"],compiler);
    assert.equal(incomplete.code,1,JSON.stringify(incomplete));
    assert.ok(incomplete.report.diagnostics.some(d=>d.code==="EC019"));
    writeFileSync(join(root,"case.test.mjs"),source.replace("// extra reach","submit(1);"));
    const complete=run(root,["verify","--runner","bun"],compiler);
    assert.equal(complete.code,0,JSON.stringify(complete));
    assert.ok(complete.report.contracts[0].paths.every(p=>p.reached===true));
  });

  test(`TS ${compiler.major}: scan facts and Rust propagation agree with native snapshots`, (t) => {
    for (const files of [
      {
        "leaf.ts": fixture("leaf.ts"),
        "api.ts": fixture("api.ts"),
        "barrel.ts":
          "export { leaf as renamed } from './leaf'; export type { Failure as PublicFailure } from './leaf';",
      },
      { "async.ts": fixture("async.ts") },
      { "result.ts": fixture("result.ts") },
      { "bindings.ts": fixture("bindings.ts") },
      { "thenables.ts": fixture("thenables.ts") },
      {
        "unicode.ts":
          "\uFEFF// 日本🧪\r\nexport type Failure={_tag:'圏外'};\r\nexport function failure():never { throw new Error('x'); }\r\nexport const alias=()=>failure();\r\n",
      },
    ]) {
      const root = project(t, files);
      const native = run(root, ["scan"]),
        official = run(root, ["scan"], compiler);
      assert.equal(official.code, 0, JSON.stringify(official));
      assert.equal(official.report.analysis.engine, "typescript");
      assert.equal(official.report.analysis.package, compiler.path);
      assert.deepEqual(
        semantic(official),
        native,
        Object.keys(files).join(","),
      );
    }
  });

  test(`TS ${compiler.major}: generic tags, aliases, path mappings and baseline round trip`, (t) => {
    const root = project(
      t,
      {
        "types.ts":
          "export type E<T extends string>={_tag:T}&{message:string}; export type Failure=E<'Offline'|'Denied'>|never;",
        "errors.ts": "export type { Failure } from '@errors/types';",
        "case.test.ts": "export {};",
      },
      { ...modern, paths: { "@errors/*": ["./*"] } },
    );
    config(root, ["Denied", "Offline"]);
    const result = run(root, ["check"], compiler);
    assert.equal(result.code, 0, JSON.stringify(result));
    assert.deepEqual(result.report.contracts[0].tags, ["Denied", "Offline"]);
    assert.equal(result.report.engine.name, "typescript");
    const before = readFileSync(join(root, "tsconfig.json"), "utf8");
    assert.equal(
      run(root, ["snapshot", "--out", "baseline.json"], compiler).code,
      0,
    );
    assert.equal(
      run(root, ["check", "--baseline", "baseline.json"], compiler).code,
      0,
    );
    assert.equal(readFileSync(join(root, "tsconfig.json"), "utf8"), before);
  });

  test(`TS ${compiler.major}: compiler errors and noCheck stop before a runner starts`, (t) => {
    for (const options of [modern, { ...modern, noCheck: true }]) {
      const root = project(
        t,
        {
          "errors.ts":
            "export type Failure={_tag:'Offline'}; const broken:number='x';",
          "case.test.ts": "export {};",
        },
        options,
      );
      config(root);
      const result = run(
        root,
        ["verify", "--vitest", "/does-not-exist"],
        compiler,
      );
      assert.equal(result.code, 2);
      assert.equal(result.report.analysisComplete, false);
      assert.equal(result.report.testsExecuted, false);
      assert.ok(
        result.report.diagnostics.some(
          (d) => d.code === (options.noCheck ? "EC_CONFIG" : "TS2322"),
        ),
        JSON.stringify(result),
      );
      assert.ok(!result.report.diagnostics.some((d) => d.code === "EC_RUNNER"));
      assert.equal(result.report.analysis.engine, "typescript");
    }
  });

  test(`TS ${compiler.major}: init generates tags without changing compiler options`, (t) => {
    const root = project(t, {
      "errors.ts": "export type Failure={_tag:'Offline'}|{_tag:'Denied'};",
    });
    const result = run(
      root,
      ["init", "--source", "errors.ts", "--export", "Failure"],
      compiler,
    );
    assert.equal(result.code, 0, JSON.stringify(result));
    assert.deepEqual(result.report.tags, ["Denied", "Offline"]);
    assert.equal(result.report.testsExecuted, false);
    assert.equal(result.report.ready, false);
    assert.ok(
      readFileSync(join(root, "errspec.generated.test.ts"), "utf8").includes(
        "throw new Error",
      ),
    );
  });

  test(`TS ${compiler.major}: EC015/16 use the same static judgments and byte locations`, (t) => {
    const root = project(t, {
      "sdk/index.d.mts": readFileSync(
        resolve(repo, "packages/testkit/index.d.mts"),
        "utf8",
      ),
      "errors.ts": "export type Failure={_tag:'Offline'};",
      "runner.ts":
        "declare const test:(name:string,body:()=>Promise<void>)=>void; declare const describe:(name:string,body:()=>void)=>void; export {test,describe};",
      "barrel.ts": "export {scenarioTest as scenario} from './sdk/index.mjs';",
      "case.test.ts": `// 日本🧪\r\nimport {test,describe} from './runner';
import {scenario} from './barrel';
describe('group',()=>scenario(test,'file:///x',{contract:'c',id:'offline',tag:'Wrong',withoutFaults:true},async s=>{
 await s.fault('typo',async()=>1)(); s.check('wrong',1,1);
}));`,
    });
    writeFileSync(
      join(root, "errspec.json"),
      JSON.stringify({
        schemaVersion: 2,
        project: "tsconfig.json",
        contracts: [
          {
            id: "c",
            source: {
              file: "errors.ts",
              export: "Failure",
              discriminant: "_tag",
            },
            cases: [
              {
                id: "offline",
                tag: "Offline",
                expect: "reject",
                test: { file: "case.test.ts" },
                observe: { faults: ["gateway"], assertions: ["status"] },
              },
            ],
          },
        ],
      }),
    );
    const native = run(root, ["check"]),
      official = run(root, ["check"], compiler);
    assert.equal(official.code, 1, JSON.stringify(official));
    assert.deepEqual(semantic(official), native);
    assert.deepEqual(
      official.report.diagnostics.map((d) => d.code),
      ["EC015", "EC015", "EC016", "EC016"],
    );
  });

  test(`TS ${compiler.major}: real Vitest verification and assertion coverage`, (t) => {
    const root = project(t, {
      "errors.ts": "export type Failure={_tag:'Offline'};",
    });
    mkdirSync(join(root, "node_modules"));
    symlinkSync(
      resolve(repo, "examples/checkout/node_modules/vitest"),
      join(root, "node_modules/vitest"),
      "dir",
    );
    const sdk = pathToFileURL(resolve(repo, "packages/testkit/index.mjs")).href;
    writeFileSync(
      join(root, "case.test.mjs"),
      `import {test,expect} from 'vitest';
import {scenarioTest} from ${JSON.stringify(sdk)};
scenarioTest(test,import.meta.url,{contract:'c',id:'offline',tag:'Offline'},async s=>{
  const fail=s.fault('gateway',async()=>({_tag:'Offline'}));
  const result=await fail(); s.check('status',result._tag,'Offline');
  expect(result).toEqual({_tag:'Offline'});
});`,
    );
    writeFileSync(
      join(root, "vitest.config.mjs"),
      "export default {test:{include:['*.test.mjs']}};",
    );
    writeFileSync(
      join(root, "errspec.json"),
      JSON.stringify({
        schemaVersion: 2,
        project: "tsconfig.json",
        contracts: [
          {
            id: "c",
            source: {
              file: "errors.ts",
              export: "Failure",
              discriminant: "_tag",
            },
            cases: [
              {
                id: "offline",
                tag: "Offline",
                expect: "reject",
                test: { file: "case.test.mjs" },
                observe: { faults: ["gateway"], assertions: ["status"] },
              },
            ],
          },
        ],
      }),
    );
    const vitest = resolve(repo, "examples/checkout/node_modules/.bin/vitest");
    const verified = run(root, ["verify", "--vitest", vitest], compiler);
    assert.equal(verified.code, 0, JSON.stringify(verified));
    assert.equal(verified.report.testsExecuted, true);
    assert.equal(
      verified.report.contracts[0].cases[0].observationStatus,
      "passed",
    );
    const coverage = run(
      root,
      ["coverage", "--vitest", vitest, "--fail-on-missing"],
      compiler,
    );
    assert.equal(coverage.code, 0, JSON.stringify(coverage));
    assert.equal(coverage.report.summary.asserted, 1);
    assert.equal(coverage.report.analysis.engine, "typescript");
  });
}

test("auto selects the nearest installed TS 6 and preserves node10/baseUrl", (t) => {
  for (const options of [
    {
      ...modern,
      module: "CommonJS",
      moduleResolution: "node",
      ignoreDeprecations: "6.0",
    },
    { ...modern, baseUrl: ".", ignoreDeprecations: "6.0" },
  ]) {
    const root = project(
      t,
      {
        "errors.ts": "export type Failure={_tag:'Offline'};",
        "case.test.ts": "export {};",
      },
      options,
    );
    config(root);
    mkdirSync(join(root, "node_modules"));
    symlinkSync(
      compilers[0].path,
      join(root, "node_modules/typescript"),
      "dir",
    );
    const before = readFileSync(join(root, "tsconfig.json"), "utf8");
    const result = run(root, ["check"], null, [
      "--analysis-node",
      process.execPath,
    ]);
    assert.equal(result.code, 0, JSON.stringify(result));
    assert.match(result.report.analysis.reason, /installed TypeScript/);
    assert.equal(result.report.engine.revision, "6.0.3");
    assert.equal(readFileSync(join(root, "tsconfig.json"), "utf8"), before);
    const native = run(root, ["check"], null, ["--analysis-engine", "tsrs"]);
    assert.equal(native.code, options.moduleResolution === "node" ? 0 : 2, JSON.stringify(native));
    assert.equal(native.report.analysis.engine, "tsrs");
  }
});

test("selected API failures and missing packages are incomplete without native retries", (t) => {
  const root = project(t, {
    "errors.ts": "export type Failure={_tag:'Offline'};",
    "case.test.ts": "export {};",
  });
  config(root);
  mkdirSync(join(root, "broken"));
  writeFileSync(
    join(root, "broken/package.json"),
    JSON.stringify({
      name: "typescript",
      version: "7.0.2",
      type: "module",
      exports: { ".": "./index.js" },
    }),
  );
  for (const flags of [
    ["--analysis-engine", "typescript"],
    ["--typescript-package", "missing"],
    ["--typescript-package", "broken", "--analysis-node", process.execPath],
    [
      "--typescript-package",
      compilers[0].path,
      "--analysis-node",
      "/missing-node",
    ],
  ]) {
    const result = run(root, ["check"], null, flags);
    assert.equal(result.code, 2, JSON.stringify(result));
    assert.equal(result.report.analysisComplete, false);
    assert.equal(result.report.diagnostics[0].code, "EC_ANALYSIS_BACKEND");
  }
});

test("auto selects TS 7 from a parent directory and explicit paths use the invoking directory", (t) => {
  const root = project(t, {
    "app/errors.ts": "export type Failure={_tag:'Offline'};",
  });
  mkdirSync(join(root, "node_modules"));
  symlinkSync(compilers[1].path, join(root, "node_modules/typescript"), "dir");
  writeFileSync(
    join(root, "app/tsconfig.json"),
    JSON.stringify({ compilerOptions: modern, files: ["errors.ts"] }),
  );
  const auto = run(root, ["scan", "--project", "app/tsconfig.json"], null, [
    "--analysis-node",
    process.execPath,
  ]);
  assert.equal(auto.code, 0, JSON.stringify(auto));
  assert.equal(auto.report.analysis.version, "7.0.2");
  symlinkSync(process.execPath, join(root, "node shim"));
  const explicit = run(root, ["scan", "--project", "app/tsconfig.json"], null, [
    "--typescript-package",
    "node_modules/typescript",
    "--analysis-node",
    "./node shim",
  ]);
  assert.equal(explicit.code, 0, JSON.stringify(explicit));
  assert.deepEqual(explicit.report.candidates, auto.report.candidates);
});

test("invalid source types stay incomplete on both official backends", (t) => {
  for (const source of [
    "any",
    "unknown",
    "{_tag:string}",
    "{_tag?:'Offline'}",
  ]) {
    const root = project(t, {
      "errors.ts": `export type Failure=${source};`,
      "case.test.ts": "export {};",
    });
    config(root);
    for (const compiler of compilers) {
      const result = run(root, ["check"], compiler);
      assert.equal(result.code, 2, JSON.stringify(result));
      assert.equal(result.report.analysisComplete, false);
      assert.equal(result.report.diagnostics[0].code, "EC002");
    }
  }
});

test("compiler diagnostics retain messages and byte locations with Unicode, BOM and CRLF", (t) => {
  const root = project(t, {
    "errors.ts":
      "\uFEFF// 日本🧪\r\nexport type Failure={_tag:'Offline'};\r\nexport const value:number='oops';",
  });
  const native = run(root, ["scan"]).report.diagnostics;
  assert.equal(native[0].code, "TS2322");
  for (const compiler of compilers) {
    const result = run(root, ["scan"], compiler);
    assert.equal(result.code, 2);
    assert.deepEqual(result.report.diagnostics, native);
  }
});

test("broken bridge output cannot become a successful empty result", (t) => {
  const root = project(t, {
    "errors.ts": "export type Failure={_tag:'Offline'};",
    "case.test.ts": "export {};",
  });
  config(root);
  const executable = join(root, "fake-node");
  for (const stdout of [
    "not JSON",
    JSON.stringify({
      protocol: 1,
      engine: "typescript",
      version: "6.0.3",
      diagnostics: [],
      tags: [],
      facts: null,
      scenarios: [],
    }),
  ]) {
    writeFileSync(executable, `#!/bin/sh\nprintf '%s\\n' '${stdout}'\n`);
    chmodSync(executable, 0o755);
    const result = run(root, ["check"], null, [
      "--typescript-package",
      compilers[0].path,
      "--analysis-node",
      executable,
    ]);
    assert.equal(result.code, 2);
    assert.equal(result.report.analysisComplete, false);
    assert.equal(result.report.diagnostics[0].code, "EC_ANALYSIS_BACKEND");
  }
});

test("native fallback needs no Node and mutation rejects semantic analysis flags", (t) => {
  const root = project(t, {
    "errors.ts": "export type Failure={_tag:'Offline'};",
    "case.test.ts": "export {};",
  });
  config(root);
  mkdirSync(join(root, "node_modules/typescript"), { recursive: true });
  writeFileSync(
    join(root, "node_modules/typescript/package.json"),
    JSON.stringify({ name: "typescript", version: "5.9.3" }),
  );
  const native = run(root, ["check"], null, [
    "--analysis-node",
    "/missing-node",
  ]);
  assert.equal(native.code, 0, JSON.stringify(native));
  assert.equal(native.report.engine.name, "tsrs");
  assert.equal(native.report.analysis, undefined);
  const explicit = run(root, ["check"], null, [
    "--analysis-engine",
    "typescript",
  ]);
  assert.equal(explicit.code, 2);
  assert.match(
    explicit.report.diagnostics[0].message,
    /Unsupported TypeScript 5/,
  );
  for (const flags of [
    ["--analysis-engine", "auto"],
    ["--typescript-package", compilers[0].path],
    ["--analysis-node", process.execPath],
  ]) {
    const mutation = run(root, ["mutate"], null, flags);
    assert.equal(mutation.code, 2);
    assert.equal(mutation.report.runComplete, false);
    assert.equal(mutation.report.diagnostics[0].code, "EC_MUTATION_CONFIG");
  }
});

for (const signal of ["SIGINT", "SIGTERM"])
  test(
    `${signal} reaps analysis workers and temporary state`,
    { timeout: 15000 },
    async (t) => {
      const root = project(t, {
        "errors.ts": "export type Failure={_tag:'Offline'};",
        "case.test.ts": "export {};",
      });
      config(root);
      const temp = join(root, "tmp");
      mkdirSync(temp);
      const executable = join(root, "fake-node");
      writeFileSync(
        executable,
        `#!${process.execPath}
import {spawn} from 'node:child_process'; import {writeFileSync} from 'node:fs';
spawn(process.execPath,['-e',"const fs=require('node:fs'); setInterval(()=>fs.appendFileSync('heartbeat','x'),10)"],{stdio:'ignore'});
writeFileSync('started','ready');setInterval(()=>{},1000);
`,
      );
      chmodSync(executable, 0o755);
      const child = spawn(
        binary,
        [
          "check",
          "--typescript-package",
          compilers[0].path,
          "--analysis-node",
          executable,
          "--format",
          "json",
        ],
        {
          cwd: root,
          env: { ...process.env, TMPDIR: temp },
          stdio: ["ignore", "pipe", "pipe"],
        },
      );
      let stdout = "",
        stderr = "";
      child.stdout.on("data", (chunk) => (stdout += chunk));
      child.stderr.on("data", (chunk) => (stderr += chunk));
      const done = new Promise((resolve, reject) => {
        child.on("error", reject);
        child.on("close", (code) => resolve(code));
      });
      t.after(() => {
        if (child.exitCode === null) child.kill("SIGKILL");
      });
      const deadline = Date.now() + 5000;
      while (!existsSync(join(root, "heartbeat")) && Date.now() < deadline)
        await delay(20);
      assert.ok(existsSync(join(root, "started")), stderr);
      assert.ok(
        existsSync(join(root, "heartbeat")),
        "API worker did not start",
      );
      child.kill(signal);
      assert.equal(await done, 2, stdout + stderr);
      const result = JSON.parse(stdout);
      assert.equal(result.analysisComplete, false);
      assert.match(result.diagnostics[0].message, /interrupted/);
      const heartbeat = readFileSync(join(root, "heartbeat"), "utf8");
      await delay(150);
      assert.equal(
        readFileSync(join(root, "heartbeat"), "utf8"),
        heartbeat,
        "Analysis worker survived its parent",
      );
      assert.deepEqual(readdirSync(temp), []);
    },
  );
