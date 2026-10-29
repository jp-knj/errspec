import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { scenarioTest, scenarioTestName } from "../packages/testkit/index.mjs";

function register(body, id = "one") {
  let callback;
  scenarioTest((_name, run) => { callback = run; }, import.meta.url, { contract: "x", id, tag: "A" }, body);
  return callback;
}

test("SDK output matches the observation fixture consumed by Rust", async () => {
  const expected = JSON.parse(readFileSync(new URL("./fixtures/observation-v1.json", import.meta.url), "utf8"));
  const directory = mkdtempSync(join(tmpdir(), "errspec-protocol-"));
  const previous = [process.env.ERRSPEC_OBSERVATIONS, process.env.ERRSPEC_RUN_ID];
  process.env.ERRSPEC_OBSERVATIONS = directory;
  process.env.ERRSPEC_RUN_ID = expected.runId;
  try {
    let callback;
    scenarioTest((_name, run) => { callback = run; }, import.meta.url, {
      contract: expected.contractId, id: expected.scenarioId, tag: expected.tag,
    }, async s => {
      const request = s.fault("gateway", async () => ({ status: 429 }));
      await request();
      s.check("status", (await request()).status, 429);
    });
    await callback();
    const files = readdirSync(directory);
    assert.equal(files.length, 1);
    const actual = JSON.parse(readFileSync(join(directory, files[0]), "utf8"));
    assert.equal(actual.file, fileURLToPath(import.meta.url));
    assert.deepEqual({ ...actual, file: "scenario.test.ts" }, expected);
  } finally {
    ["ERRSPEC_OBSERVATIONS", "ERRSPEC_RUN_ID"].forEach((key, i) => {
      if (previous[i] === undefined) delete process.env[key]; else process.env[key] = previous[i];
    });
    rmSync(directory, { recursive: true, force: true });
  }
});

test("fault observes asynchronous invocations and equality checks", async () => {
  await register(async s => {
    const fail = s.fault("gateway", async n => ({ error: n }));
    s.check("value", await fail(42), { error: 42 });
  })();
  assert.equal(scenarioTestName('x"日本語', "a/b"), 'errspec ["x\\"日本語","a/b"]');
});

test("configured but unused faults cannot pass", async () => {
  await assert.rejects(register(s => {
    s.fault("gateway", () => ({ error: "A" }));
    s.check("status", 429, 429);
  }), /invoked and settled/);
});

test("unfinished fault promises and absent checks cannot pass", async () => {
  await assert.rejects(register(s => {
    void s.fault("gateway", () => new Promise(() => {}))();
    s.check("status", 429, 429);
  }), /invoked and settled/);
  await assert.rejects(register(async s => { await s.fault("gateway", () => 1)(); }), /no completed assertions/);
});

test("caught failed comparisons remain failures", async () => {
  await assert.rejects(register(async s => {
    await s.fault("gateway", () => 1)();
    try { s.check("status", 200, 429); } catch { /* application code cannot erase evidence */ }
  }), /failed check/);
});

test("duplicate ids and promise comparisons remain failures when caught", async () => {
  for (const operation of [
    s => { s.check("status", 1, 1); s.check("status", 1, 1); },
    s => s.fault("gateway", () => 2),
    s => { const p = Promise.resolve(1); s.check("status", p, p); },
  ]) {
    await assert.rejects(register(async s => {
      await s.fault("gateway", () => 1)();
      try { operation(s); } catch {}
    }), /failed check/);
  }
});

test("a rejection can be injected and checked by the application", async () => {
  await register(async s => {
    const fail = s.fault("network", () => { throw new Error("offline"); });
    let message;
    try { await fail(); } catch (error) { message = error.message; }
    s.check("message", message, "offline");
  })();
});

test("concurrent scopes have separate complete records; late checks invalidate evidence", async () => {
  const directory = mkdtempSync(join(tmpdir(), "errspec-sdk-"));
  const previous = [process.env.ERRSPEC_OBSERVATIONS, process.env.ERRSPEC_RUN_ID];
  process.env.ERRSPEC_OBSERVATIONS = directory;
  process.env.ERRSPEC_RUN_ID = "unit-run";
  let captured;
  try {
    await Promise.all(["one", "two"].map(id => register(async s => {
      if (id === "one") captured = s;
      s.check("result", await s.fault("gateway", async () => id)(), id);
    }, id)()));
    let records = readdirSync(directory).map(f => JSON.parse(readFileSync(join(directory, f), "utf8")));
    assert.deepEqual(records.map(r => r.scenarioId).sort(), ["one", "two"]);
    assert(records.every(r => r.state === "passed" && r.runId === "unit-run" && r.faults[0].completed === 1));
    assert.throws(() => captured.check("late", 1, 1), /already finished/);
    records = readdirSync(directory).map(f => JSON.parse(readFileSync(join(directory, f), "utf8")));
    assert.equal(records.find(r => r.scenarioId === "one").state, "incomplete");
  } finally {
    ["ERRSPEC_OBSERVATIONS", "ERRSPEC_RUN_ID"].forEach((key, i) => {
      if (previous[i] === undefined) delete process.env[key]; else process.env[key] = previous[i];
    });
    rmSync(directory, { recursive: true, force: true });
  }
});
