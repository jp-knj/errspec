// Reproduce the adoption experiment against actual, pinned Valibot source.
// The supplied checkout is read through git archive and is never modified.
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { resolve, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import assert from "node:assert/strict";

const revision = "d72f76fcf70ccfa0a5ca341b80da1d440ac60b11";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const checkout = process.argv[2] && resolve(process.argv[2]);
if (!checkout)
  throw new Error(
    "Usage: node scripts/pilot-valibot.mjs <valibot-checkout> [errspec-binary]",
  );
const binary = resolve(process.argv[3] || join(root, "target/debug/errspec"));
const dir = mkdtempSync(join(tmpdir(), "errspec-valibot-"));
const checks = [];
const timings = [];
function run(command, args, expected = 0) {
  const start = performance.now();
  const r = spawnSync(command, args, {
    cwd: dir,
    encoding: "utf8",
    timeout: 120000,
    maxBuffer: 32 * 1024 * 1024,
  });
  timings.push({
    command: command === binary ? "errspec" : command,
    args,
    milliseconds: Math.round(performance.now() - start),
  });
  assert.equal(
    r.status,
    expected,
    `${command} ${args.join(" ")}\n${r.stdout}\n${r.stderr}\n${r.error || ""}`,
  );
  return r.stdout;
}
function cli(args, expected = 0) {
  return JSON.parse(run(binary, [...args, "--format", "json"], expected));
}
function has(report, code) {
  return report.diagnostics.some((d) => d.code === code);
}
try {
  const archive = spawnSync(
    "git",
    ["-C", checkout, "archive", revision, "library", "LICENSE.md"],
    { maxBuffer: 128 * 1024 * 1024 },
  );
  assert.equal(archive.status, 0, archive.stderr?.toString());
  const unpack = spawnSync("tar", ["-x", "-C", dir], { input: archive.stdout });
  assert.equal(unpack.status, 0, unpack.stderr?.toString());
  writeFileSync(
    join(dir, "package.json"),
    JSON.stringify({
      name: "errspec-valibot-pilot",
      private: true,
      type: "module",
    }),
  );
  run("npm", [
    "install",
    "--offline",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    `file:${join(root, "packages/testkit")}`,
  ]);
  writeFileSync(
    join(dir, "errspec.tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        strict: true,
        target: "ES2022",
        module: "ESNext",
        moduleResolution: "Bundler",
        allowImportingTsExtensions: true,
        noEmit: true,
        skipLibCheck: true,
      },
      files: ["library/src/schemas/string/string.ts"],
    }),
  );
  const scan = cli([
    "scan",
    "--project",
    "errspec.tsconfig.json",
    "--discriminant",
    "type",
  ]);
  assert(
    scan.candidates.some(
      (c) => c.source.export === "StringIssue" && c.tags.includes("string"),
    ),
  );
  checks.push({ name: "discover actual StringIssue", passed: true });
  cli([
    "init",
    "--project",
    "errspec.tsconfig.json",
    "--source",
    "library/src/schemas/string/string.ts",
    "--export",
    "StringIssue",
    "--discriminant",
    "type",
    "--runner",
    "bun",
    "--contract",
    "valibot.string",
  ]);
  const verify = ["verify", "--runner", "bun", "--only-contract-tests"];
  const todo = cli(verify, 1);
  assert.equal(todo.ok, false);
  checks.push({ name: "generated TODO fails", passed: true });
  const config = JSON.parse(readFileSync(join(dir, "errspec.json"), "utf8"));
  config.contracts[0].cases[0].expect =
    "Reject a number with a string issue and preserve the offending input";
  config.contracts[0].cases[0].observe.assertions = [
    "rejected",
    "issue-type",
    "input",
  ];
  const saveConfig = (c) =>
    writeFileSync(join(dir, "errspec.json"), JSON.stringify(c, null, 2));
  saveConfig(config);
  const test = `import { test } from 'bun:test';
import { scenarioTest } from '@errspec/testkit';
import { string } from './library/src/schemas/string/string.ts';
import { safeParse } from './library/src/methods/safeParse/safeParse.ts';

scenarioTest(test, import.meta.url, { contract: 'valibot.string', id: 'case-1', tag: 'string' }, async (s) => {
  const input = s.fault('dependency', async () => 123);
  const result = safeParse(string(), await input());
  s.check('rejected', result.success, false);
  if (result.success) throw new Error('Expected invalid input');
  const issue = result.issues[0];
  s.check('issue-type', issue.type, 'string');
  s.check('input', issue.input, 123);
});
`;
  const file = join(dir, "errspec.generated.test.ts");
  writeFileSync(file, test);
  assert.equal(cli(verify).ok, true);
  checks.push({ name: "real Valibot failure behavior passes", passed: true });
  cli(["snapshot", "--out", "baseline.json"]);
  const strict = [
    ...verify,
    "--baseline",
    "baseline.json",
    "--strict-baseline",
  ];
  assert.equal(cli(strict).ok, true);
  const missing = structuredClone(config);
  missing.contracts[0].cases = [];
  saveConfig(missing);
  assert(has(cli(["check"], 1), "EC001"));
  saveConfig(config);
  checks.push({ name: "missing registration detected: EC001", passed: true });
  writeFileSync(
    file,
    test.replace("  s.check('input', issue.input, 123);\n", ""),
  );
  run("bun", ["test", "./errspec.generated.test.ts"]);
  assert(has(cli(verify, 1), "EC010"));
  checks.push({
    name: "ordinary test passes but missing comparison fails: EC010",
    passed: true,
  });
  const weaker = structuredClone(config);
  weaker.contracts[0].cases[0].observe.assertions =
    weaker.contracts[0].cases[0].observe.assertions.filter(
      (x) => x !== "input",
    );
  saveConfig(weaker);
  assert.equal(cli(verify).ok, true);
  const changed = cli(strict, 1);
  assert(has(changed, "EC014"));
  checks.push({
    name: "removing comparison and requirement fails strict baseline: EC014",
    passed: true,
  });
  saveConfig(config);
  writeFileSync(file, test);
  assert.equal(cli(strict).ok, true);
  checks.push({
    name: "restored behavior and reviewed baseline pass",
    passed: true,
  });
  const result = {
    project: "https://github.com/open-circle/valibot",
    revision,
    node: process.version,
    bun: run("bun", ["--version"]).trim(),
    source: "library/src/schemas/string/string.ts",
    export: "StringIssue",
    discriminant: "type",
    sourceFilesModified: 0,
    setupFiles: [
      "package.json",
      "package-lock.json",
      "errspec.tsconfig.json",
      "errspec.json",
      "errspec.generated.test.ts",
      "baseline.json",
    ],
    scan: {
      files: scan.files.length,
      candidates: scan.candidates.length,
      origins: scan.origins.length,
      unknowns: scan.unknowns.length,
    },
    checks,
    timings,
    limits: [
      "One schema and one scenario; not a benchmark of all Valibot behavior.",
      "No claim about human setup time or long-term maintenance.",
      "The test tsconfig focuses on source; runner type dependencies are not analyzed.",
    ],
  };
  mkdirSync(join(root, "artifacts"), { recursive: true });
  writeFileSync(
    join(root, "artifacts/valibot-pilot.json"),
    JSON.stringify(result, null, 2) + "\n",
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  rmSync(dir, { recursive: true, force: true });
}
