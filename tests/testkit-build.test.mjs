import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../", import.meta.url));
const artifacts = ["index.mjs", "index.d.mts"];
function project(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "errspec-testkit-build-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const file of ["scripts/build-testkit.mjs", "packages/analysis-driver/package.json", ...["index.ts", ...artifacts].map(name => `packages/testkit/${name}`)]) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    copyFileSync(join(repo, file), join(root, file));
  }
  symlinkSync(join(repo, "packages/analysis-driver/node_modules"), join(root, "packages/analysis-driver/node_modules"), "dir");
  return root;
}
function build(root, check = false) {
  return spawnSync(process.execPath, [join(root, "scripts/build-testkit.mjs"), ...(check ? ["--check"] : [])], {
    cwd: tmpdir(), encoding: "utf8", timeout: 30_000,
  });
}
function bytes(root, name) { return readFileSync(join(root, "packages/testkit", name)); }

test("committed testkit artifacts match the canonical TypeScript source", () => {
  const result = build(repo, true);
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
});

test("check detects each stale artifact without repairing it", t => {
  const root = project(t);
  for (const name of artifacts) {
    const original = bytes(root, name);
    writeFileSync(join(root, "packages/testkit", name), "stale artifact\n");
    const result = build(root, true);
    assert.notEqual(result.status, 0);
    assert.ok(result.stderr.includes(name), result.stderr);
    assert.equal(bytes(root, name).toString(), "stale artifact\n");
    writeFileSync(join(root, "packages/testkit", name), original);
  }
});

test("generation follows source changes and is independent of checkout location", t => {
  const roots = [project(t), project(t)];
  for (const root of roots) {
    const path = join(root, "packages/testkit/index.ts");
    writeFileSync(path, readFileSync(path, "utf8") + '\nexport const generationProbe: string = "source changed";\n');
    assert.notEqual(build(root, true).status, 0);
    const result = build(root);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(build(root, true).status, 0);
    assert.match(bytes(root, "index.mjs").toString(), /generationProbe = "source changed"/);
    assert.match(bytes(root, "index.d.mts").toString(), /generationProbe: string/);
  }
  for (const name of artifacts) assert.deepEqual(bytes(roots[0], name), bytes(roots[1], name));
});

test("invalid canonical source does not overwrite committed artifacts", t => {
  const root = project(t), before = artifacts.map(name => bytes(root, name));
  writeFileSync(join(root, "packages/testkit/index.ts"), "export const invalid = ;\n");
  const result = build(root);
  assert.notEqual(result.status, 0);
  artifacts.forEach((name, index) => assert.deepEqual(bytes(root, name), before[index]));
});
