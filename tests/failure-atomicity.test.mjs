import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const entry = new URL(process.env.TESTKIT_ENTRY ?? "../packages/testkit/index.mjs", import.meta.url);
const { checkFailureAtomicity, FailureAtomicityError } = await import(entry.href);
const down = { id: "StoreDown", dependency: "save", inject: () => ({ ok: false }) };
const isFailure = result => !result.ok;

function options({ rollback = true, twice = false, faults = [down], only } = {}) {
  let created = 0;
  let disposed = 0;
  return {
    counts: () => [created, disposed],
    options: {
      isFailure, faults, only,
      setup() {
        created++;
        const state = { balance: 10 };
        return {
          snapshot: () => state, // Deliberately returns the same mutable object.
          async run(scope) {
            const save = scope.wrap("save", async () => ({ ok: true }));
            state.balance--;
            for (let i = 0; i < (twice ? 2 : 1); i++) {
              const result = await save();
              if (!result.ok) {
                if (rollback) state.balance++;
                return result;
              }
            }
            return { ok: true };
          },
          dispose() { disposed++; },
        };
      },
    },
  };
}

test("finds a partial write in unchanged application code and accepts its rollback fix", async () => {
  const broken = options({ rollback: false });
  await assert.rejects(checkFailureAtomicity(broken.options), error => {
    assert(error instanceof FailureAtomicityError);
    assert.match(error.message, /StoreDown at save call 1/);
    const [trial] = error.report.cases;
    assert.equal(trial.status, "violated");
    assert.deepEqual(trial.before, { balance: 10 });
    assert.deepEqual(trial.after, { balance: 9 });
    return true;
  });
  assert.deepEqual(broken.counts(), [2, 2]);
  const fixed = options();
  const report = await checkFailureAtomicity(fixed.options);
  assert.deepEqual({ ...report.baseline.calls }, { save: 1 });
  assert.equal(report.cases[0].status, "preserved");
  assert.deepEqual(fixed.counts(), [2, 2]);
});

test("explores each call independently and replays only the named case", async () => {
  const all = options({ twice: true });
  const report = await checkFailureAtomicity(all.options);
  assert.deepEqual(report.cases.map(trial => trial.fault.call), [1, 2]);
  assert.deepEqual(all.counts(), [3, 3]);
  const replay = options({ twice: true, only: report.cases[1].fault });
  const one = await checkFailureAtomicity(replay.options);
  assert.equal(one.cases.length, 1);
  assert.equal(one.cases[0].fault.call, 2);
  assert.deepEqual(replay.counts(), [2, 2]);
});

test("collects multiple counterexamples instead of stopping at the first", async () => {
  const fixture = options({ rollback: false, faults: [down, { ...down, id: "StoreBusy" }] });
  await assert.rejects(checkFailureAtomicity(fixture.options), error => {
    assert.deepEqual(error.report.cases.map(trial => trial.fault.fault), ["StoreDown", "StoreBusy"]);
    assert(error.report.cases.every(trial => trial.status === "violated"));
    return true;
  });
  assert.deepEqual(fixture.counts(), [3, 3]);
});

test("distinguishes successful retry from verified failure atomicity", async () => {
  const report = await checkFailureAtomicity({
    faults: [down], isFailure,
    setup: () => ({
      snapshot: () => ({}),
      async run(scope) {
        const save = scope.wrap("save", async () => ({ ok: true }));
        const first = await save();
        return first.ok ? first : save();
      },
    }),
  });
  assert.equal(report.cases[0].status, "recovered");
});

test("preserves arguments and this, including in the injected implementation", async () => {
  const seen = [];
  const report = await checkFailureAtomicity({
    faults: [{ ...down, inject(n) { seen.push([this.label, n]); return { ok: false }; } }], isFailure,
    setup: () => ({
      snapshot: () => null,
      run(scope) {
        const object = { label: "store", save: scope.wrap("save", function (n) {
          seen.push([this.label, n]); return { ok: true };
        }) };
        return object.save(42);
      },
    }),
  });
  assert.deepEqual(seen, [["store", 42], ["store", 42]]);
  assert.equal(report.cases[0].status, "preserved");
});

test("supports injected rejections handled by the application, but rejects unexpected throws", async () => {
  let disposed = 0;
  const make = handled => ({
    faults: [{ ...down, inject() { throw new Error("offline"); } }], isFailure,
    setup: () => ({
      snapshot: () => null,
      async run(scope) {
        try { return await scope.wrap("save", () => ({ ok: true }))(); }
        catch (error) { if (!handled) throw error; return { ok: false }; }
      },
      dispose() { disposed++; },
    }),
  });
  assert.equal((await checkFailureAtomicity(make(true))).cases[0].status, "preserved");
  await assert.rejects(checkFailureAtomicity(make(false)), /StoreDown at save call 1: offline/);
  assert.equal(disposed, 4);
});

test("never passes an empty matrix, an unknown dependency or an impossible replay", async () => {
  for (const faults of [[], [down, down]]) {
    await assert.rejects(checkFailureAtomicity(options({ faults }).options), TypeError);
  }
  await assert.rejects(checkFailureAtomicity(options({ faults: [{ ...down, dependency: "unused" }] }).options), /not observed/);
  await assert.rejects(checkFailureAtomicity(options({ only: { fault: "StoreDown", call: 2 } }).options), /not observed/);
  await assert.rejects(checkFailureAtomicity(options({ only: { fault: "unknown", call: 1 } }).options), TypeError);
});

test("fails if a fresh fixture does not reach the selected injection", async () => {
  let count = 0;
  await assert.rejects(checkFailureAtomicity({
    faults: [down], isFailure,
    setup() {
      const first = count++ === 0;
      return { snapshot: () => null, run: scope => first ? scope.wrap("save", () => ({ ok: true }))() : { ok: false } };
    },
  }), /not injected/);
});

test("rejects failing baselines, unfinished calls and failed cleanup", async () => {
  await assert.rejects(checkFailureAtomicity({
    faults: [down], isFailure,
    setup: () => ({ run: () => ({ ok: false }), snapshot: () => null }),
  }), /Baseline.*must succeed/);
  let finish;
  await assert.rejects(checkFailureAtomicity({
    faults: [down], isFailure,
    setup: () => ({
      run(scope) { void scope.wrap("save", () => new Promise(resolve => { finish = resolve; }))(); return { ok: true }; },
      snapshot: () => null,
      dispose() { finish({ ok: true }); },
    }),
  }), /still pending/);
  await assert.rejects(checkFailureAtomicity({
    ...options().options,
    setup: () => ({ run: () => ({ ok: true }), snapshot: () => null, dispose() { throw new Error("cleanup"); } }),
  }), /cleanup failed/);
});

const example = new URL("../examples/checkout/", import.meta.url).pathname;
const vitest = join(example, "node_modules/.bin/vitest");
test("real Vitest reports the counterexample and passes the repaired operation", {
  skip: !existsSync(vitest) && "run npm ci in examples/checkout",
}, () => {
  const work = mkdtempSync(join(example, ".atomicity-"));
  try {
    const source = `
import { test } from "vitest";
import { checkFailureAtomicity } from ${JSON.stringify(entry.href)};
const rollback = false;
test("submit preserves state when persistence fails", async () => {
  await checkFailureAtomicity({
    faults: [{ id: "StoreDown", dependency: "save", inject: () => ({ ok: false }) }],
    isFailure: r => !r.ok,
    setup() {
      const state = { count: 0 };
      return {
        snapshot: () => state,
        async run(s) {
          state.count++;
          const r = await s.wrap("save", async () => ({ ok: true }))();
          if (!r.ok && rollback) state.count--;
          return r;
        }
      };
    }
  });
});`;
    const file = join(work, "atomicity.test.ts");
    writeFileSync(file, source);
    writeFileSync(join(work, "vitest.config.mjs"), 'export default { test: { include: ["*.test.ts"] } };');
    const run = () => spawnSync(vitest, ["run", "--config", "vitest.config.mjs"], { cwd: work, encoding: "utf8" });
    const broken = run();
    assert.equal(broken.status, 1, broken.stdout + broken.stderr);
    assert.match(broken.stdout + broken.stderr, /StoreDown at save call 1/);
    writeFileSync(file, source.replace("rollback = false", "rollback = true"));
    const fixed = run();
    assert.equal(fixed.status, 0, fixed.stdout + fixed.stderr);
  } finally { rmSync(work, { recursive: true, force: true }); }
});
