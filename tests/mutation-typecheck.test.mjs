import {temporaryProject} from "./support/project.mjs";
import assert from "node:assert/strict";
import { test } from "node:test";
import {writeFileSync,readFileSync,realpathSync,mkdirSync,symlinkSync} from "node:fs";
import { resolve, join, dirname, delimiter } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import {
  applyCandidate,
  newDiagnostic,
  typecheckMutants,
} from "../packages/mutation-driver/typecheck.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const compilers = [5, 6, 7].map((major) => ({
  major,
  package: realpathSync(
    join(repo, `packages/analysis-driver/node_modules/typescript${major}`),
  ),
}));
const binary = process.env.ERRSPEC_BIN || join(repo, "target/debug/errspec");
const modern = {
  strict: true,
  noUncheckedIndexedAccess: true,
  target: "ES2022",
  module: "ESNext",
  moduleResolution: "Bundler",
  skipLibCheck: true,
};

function project(t, source, options = modern) {
  const root = temporaryProject(t,"errspec-mutant-types-");

  mkdirSync(join(root, "src"));
  writeFileSync(join(root, "src/value.ts"), source);
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({ compilerOptions: options, files: ["src/value.ts"] }),
  );
  return root;
}
function candidate(
  source,
  original,
  replacement,
  key = "candidate",
  mutator = "Test",
) {
  const start = source.indexOf(original);
  assert.ok(start >= 0);
  return {
    key,
    original,
    replacement,
    mutator,
    byteOffset: Buffer.byteLength(source.slice(0, start)),
    end: Buffer.byteLength(source.slice(0, start + original.length)),
    ignored: null,
  };
}
function check(root, source, candidates, compiler) {
  return typecheckMutants(
    { root, typecheckPackage: compiler.package },
    new Map([[join(root, "src/value.ts"), { source, candidates }]]),
  );
}

test("diagnostic differences preserve multiplicity and candidate ranges preserve UTF-8", () => {
  assert.equal(
    newDiagnostic(["known", "known"], ["known", "known"]),
    undefined,
  );
  assert.equal(newDiagnostic(["known"], ["known", "known"]), "known");
  assert.equal(newDiagnostic(["known"], ["fresh"]), "fresh");
  const source = "\uFEFF// 日本🧪\r\nexport const n:number=42;";
  assert.equal(
    applyCandidate(source, candidate(source, "42", "'bad'")),
    source.replace("42", "'bad'"),
  );
  assert.throws(
    () =>
      applyCandidate(source, {
        ...candidate(source, "42", "0"),
        original: "wrong",
      }),
    /Invalid original/,
  );
});

for (const compiler of compilers) {
  test(`TS ${compiler.major}: mutant overlays distinguish new errors and restore original text`, async (t) => {
    const source =
      "\uFEFF// 日本🧪\r\nexport const n:number=42;\r\nconst known:string=123;\r\n";
    const root = project(t, source);
    const result = await check(
      root,
      source,
      [
        candidate(source, "42", "'bad'", "bad"),
        candidate(source, "42", "0", "valid"),
      ],
      compiler,
    );
    assert.equal(result.info.checked, 2);
    assert.equal(result.info.compileErrors, 1);
    assert.ok(result.outcomes[0].diagnostic);
    assert.match(result.outcomes[0].diagnostic, /TS2322/);
    assert.equal(result.outcomes[1].diagnostic, null);
    assert.equal(result.outcomes[1].equivalentReason, null);
    assert.equal(readFileSync(join(root, "src/value.ts"), "utf8"), source);
  });

  test(`TS ${compiler.major}: optional chains need honest indexed access and nullability`, async (t) => {
    const source =
      "export function object(x:{value:number}) { return x?.value; }\nexport function array(xs:string[]) { return xs[0]?.length; }\n";
    for (const honest of [true, false]) {
      const root = project(t, source, {
        ...modern,
        noUncheckedIndexedAccess: honest,
      });
      const result = await check(
        root,
        source,
        [
          candidate(
            source,
            "x?.value",
            "x.value",
            "object",
            "OptionalChaining",
          ),
          candidate(
            source,
            "xs[0]?.length",
            "xs[0].length",
            "array",
            "OptionalChaining",
          ),
        ],
        compiler,
      );
      assert.equal(
        result.outcomes[0].equivalentReason !== null,
        honest,
        JSON.stringify(result),
      );
      assert.equal(
        result.outcomes[1].diagnostic !== null,
        honest,
        JSON.stringify(result),
      );
      if (honest) assert.match(result.outcomes[0].equivalentReason, /equivalent:/);
    }
  });

  test(`TS ${compiler.major}: noCheck is an error rather than a successful empty check`, async (t) => {
    const source = "export const n:number=42;",
      root = project(t, source, { ...modern, noCheck: true });
    await assert.rejects(
      check(root, source, [candidate(source, "42", "'bad'")], compiler),
      /noCheck/,
    );
    assert.equal(readFileSync(join(root, "src/value.ts"), "utf8"), source);
  });

  test(`TS ${compiler.major}: CLI skips CompileError mutants and keeps the legacy path`, (t) => {
    const source = "export function value():number{return 42;}";
    const root = project(t, source);
    mkdirSync(join(root, "node_modules"));
    symlinkSync(
      join(repo, "examples/checkout/node_modules/vitest"),
      join(root, "node_modules/vitest"),
      "dir",
    );
    writeFileSync(
      join(root, "value.test.ts"),
      'import {test,expect} from "vitest"; import {value} from "./src/value";test("value",()=>expect(value()).toBe(42));',
    );
    writeFileSync(
      join(root, "vitest.config.mjs"),
      'export default {test:{include:["value.test.ts"]}};',
    );
    const run = (flags) => {
      const output = spawnSync(
        binary,
        ["mutate", "--node", process.execPath, "--format", "json", ...(process.env.ERRSPEC_REUSE_SESSION?["--reuse-session"]:[]), ...flags],
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
      assert.ifError(output.error);
      assert.equal(output.status, 0, output.stdout + output.stderr);
      return JSON.parse(output.stdout);
    };
    const typed = run(["--typecheck", "--typecheck-package", compiler.package]);
    assert.equal(typed.runComplete, true);
    assert.equal(typed.engine.typecheck, "complete");
    assert.ok(typed.summary.compileError > 0, JSON.stringify(typed));
    assert.equal(
      typed.typecheck.version,
      { 5: "5.9.3", 6: "6.0.3", 7: "7.0.2" }[compiler.major],
    );
    for (const mutant of typed.mutants.filter(
      (m) => m.status === "CompileError",
    ))
      assert.deepEqual(mutant.attempts, []);
    const legacy = run(["--no-typecheck"]);
    assert.equal(legacy.engine.typecheck, "not_run");
    assert.equal(legacy.summary.compileError, undefined);
    assert.equal(legacy.typecheck, undefined);
    assert.equal(readFileSync(join(root, "src/value.ts"), "utf8"), source);
  });
}
