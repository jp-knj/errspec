import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { checkedValues, collect, testFullName } from "../src/coverage/vitest-recorder.mjs";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const recorder = join(project, "src/coverage/vitest-recorder.mjs");
// The checkout example is the repository's only installed Vitest; CI runs npm ci there first.
const example = join(project, "examples/checkout");
const vitest = join(example, "node_modules/.bin/vitest");

test("collect keeps discriminant values and bare strings only", () => {
  const keys = new Set(["_tag", "code"]);
  const cyclic = { _tag: "Loop" };
  cyclic.self = cyclic;
  assert.deepEqual(
    collect({ ok: false, error: { _tag: "Declined", reason: "funds" }, list: ["A", 1], nested: cyclic }, keys),
    [["_tag", "Declined"], ["", "A"], ["_tag", "Loop"]],
  );
  assert.deepEqual(collect("RateLimited", keys), [["", "RateLimited"]]);
  assert.deepEqual(collect(429, keys), []);
  assert.deepEqual(collect(Object.assign(new Error("x"), { code: "E_DOWN" }), keys), [["code", "E_DOWN"]]);
});

test("partial assertions credit only tags required by their expectations", () => {
  const keys = new Set(["_tag", "code"]);
  const actual = { ok: false, error: { _tag: "Declined" } };
  assert.deepEqual(checkedValues("toMatchObject", actual, [{ ok: false }], keys), []);
  assert.deepEqual(checkedValues("toHaveProperty", actual, ["error._tag"], keys), []);
  assert.deepEqual(checkedValues("toBe", actual, [actual], keys), []);
  assert.deepEqual(checkedValues("toContain", [actual], [actual], keys), []);
  assert.deepEqual(checkedValues("toHaveProperty", actual, ["error._tag", "Declined"], keys), [["_tag", "Declined"]]);
  assert.deepEqual(checkedValues("toHaveProperty", actual, ["message", "Declined"], keys), []);
  assert.deepEqual(checkedValues("toMatchObject", actual, [{ error: { asymmetricMatch() {}, sample: { _tag: "Declined" } } }], keys), []);
  let reads = 0;
  const getter = { get _tag() { reads++; return "Declined"; } };
  assert.deepEqual(collect(getter, keys), []);
  assert.equal(reads, 0);
  assert.deepEqual(collect(Object.defineProperty({}, "_tag", { value: "Declined" }), keys), []);
  const file = { name: "file.test.ts" };
  const suite = { name: "outer > literal", suite: file };
  const task = { name: "inner > literal", suite, file };
  assert.equal(testFullName(task, "outer > literal > inner > literal"), "outer > literal inner > literal");
  assert.equal(testFullName(undefined, "fallback"), "fallback");
});

test("records only passing positive assertions from real Vitest runs", { skip: !existsSync(vitest) && "run npm ci in examples/checkout" }, () => {
  const work = mkdtempSync(join(example, ".recorder-"));
  try {
    const output = join(work, "out");
    mkdirSync(output);
    writeFileSync(join(work, "cases.test.ts"), `
import { describe, expect, test } from "vitest";
const fail = (code: string) => Object.assign(new Error(code), { code });
describe("group > literal", () => {
test("asserted", async () => {
  expect({ ok: false, error: { _tag: "Declined" } }).toEqual({ ok: false, error: { _tag: "Declined" } });
  expect({ _tag: "Absent" }).not.toEqual({ _tag: "Other" });
  try { expect({ _tag: "Wrong" }).toEqual({ _tag: "Right" }); } catch {}
  await expect(Promise.resolve({ kind: "late" })).resolves.toMatchObject({ kind: "late" });
  await expect(Promise.reject(fail("E_DOWN"))).rejects.toMatchObject({ code: "E_DOWN" });
  expect("RateLimited").toBe("RateLimited");
  expect({ ok: false, error: { _tag: "Unchecked" } }).toMatchObject({ ok: false });
  expect({ _tag: "Present" }).toHaveProperty("_tag");
  expect({ _tag: "Asymmetric" }).toEqual(expect.objectContaining({ _tag: "Asymmetric" }));
  const same = { _tag: "Identity" };
  expect(same).toBe(same);
});
});
`);
    writeFileSync(join(work, "vitest.config.mjs"), `export default { test: { include: ["*.test.ts"], setupFiles: [${JSON.stringify(recorder)}] } };`);
    const result = spawnSync(vitest, ["run", "--config", "vitest.config.mjs"], {
      cwd: work,
      encoding: "utf8",
      env: {
        ...process.env,
        ERRSPEC_COVERAGE: output,
        ERRSPEC_VITEST_PACKAGE: join(example, "node_modules/vitest"),
        ERRSPEC_DISCRIMINANTS: "_tag,kind,code",
      },
    });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const records = readdirSync(output)
      .flatMap(file => readFileSync(join(output, file), "utf8").trim().split("\n"))
      .map(line => JSON.parse(line));
    assert.deepEqual(
      records.flatMap(r => r.values).sort(),
      [["", "RateLimited"], ["_tag", "Declined"], ["code", "E_DOWN"], ["kind", "late"]].sort(),
    );
    assert(records.every(r => r.file.endsWith("cases.test.ts") && r.test === "group > literal asserted"));
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});
