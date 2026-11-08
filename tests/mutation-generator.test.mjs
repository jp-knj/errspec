import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  writeFileSync,
  chmodSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { Generator } from "../packages/mutation-driver/generator.mjs";
const binary =
  process.env.ERRSPEC_BIN ||
  new URL("../target/debug/errspec", import.meta.url).pathname;
const request = {
  file: "src/example.ts",
  source: "export const n=(x:number)=>x+1;",
  facts: true,
  weakMutation: true,
};
function fake(t, body) {
  const root = mkdtempSync(join(tmpdir(), "errspec-generator-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const path = join(root, "fake");
  writeFileSync(path, `#!${process.execPath}\n${body}`);
  chmodSync(path, 0o700);
  return path;
}
async function stopped(g) {
  await g.close();
  assert(g.child.exitCode !== null || g.child.signalCode !== null);
}
test("nonzero exit during shutdown remains a protocol failure", async (t) => {
  const g = new Generator(
    fake(
      t,
      `process.stdin.resume();process.stdin.on('end',()=>process.exit(2));`,
    ),
  );
  await stopped(g);
  assert.equal(g.failure.code, "EC_MUTATION_PROTOCOL");
  assert.equal(g.child.exitCode, 2);
});
test("one child serves repeated requests and recovers from a syntax error", async () => {
  const g = new Generator(binary);
  try {
    const first = await g.call(request),
      pid = g.child.pid;
    assert(first.candidates.length > 0);
    const single = spawnSync(binary, ["__mutation-source"], {
      input: JSON.stringify(request),
      encoding: "utf8",
    });
    assert.equal(single.status, 0, single.stderr);
    assert.deepEqual(first, JSON.parse(single.stdout));
    await assert.rejects(
      g.call({ ...request, source: "export const = ;" }),
      (e) => e.code === "EC_MUTATION_INSTRUMENTATION",
    );
    assert.deepEqual(await g.call(request), first);
    assert.equal(g.child.pid, pid);
    assert.deepEqual(
      (await g.call({ ...request, validateOnly: true })).candidates,
      [],
    );
  } finally {
    await stopped(g);
  }
});
for (const [name, body] of [
  [
    "malformed JSON",
    `process.stdin.once('data',()=>process.stdout.write('broken\\n'));`,
  ],
  [
    "wrong ID",
    `process.stdin.once('data',()=>process.stdout.write(JSON.stringify({version:1,id:99,result:{candidates:[]}})+'\\n'));`,
  ],
  ["unexpected EOF", `process.stdin.once('data',()=>process.exit(0));`],
  ["null JSON", `process.stdin.once('data',()=>console.log('null'));`],
  [
    "invalid UTF-8",
    `process.stdin.once('data',()=>process.stdout.write(Buffer.from([255,10])));`,
  ],
  [
    "wrong version",
    `process.stdin.once('data',()=>console.log(JSON.stringify({version:2,id:1,result:{candidates:[]}})));`,
  ],
  [
    "duplicate responses",
    `process.stdin.once('data',()=>process.stdout.write([1,1].map(id=>JSON.stringify({version:1,id,result:{candidates:[]}})).join(String.fromCharCode(10))+String.fromCharCode(10)));`,
  ],
  [
    "oversized response",
    `process.stdin.once('data',()=>process.stdout.write('x'.repeat(4096)));`,
  ],
  [
    "oversized stderr",
    `process.stdin.once('data',()=>process.stderr.write('x'.repeat(4096)));`,
  ],
  ["deadline", `process.stdin.resume();setInterval(()=>{},1000);`],
])
  test(`${name} is a protocol failure and reaps the child`, async (t) => {
    const g = new Generator(fake(t, body), {
      timeout: name === "deadline" ? 100 : 3000,
      maxBuffer: 2048,
    });
    const message = {
      "malformed JSON": /Invalid generator JSON/,
      "wrong ID": /Invalid generator response/,
      "unexpected EOF": /Generator exited/,
      "null JSON": /Invalid generator response/,
      "invalid UTF-8": /Invalid generator JSON/,
      "wrong version": /Invalid generator response/,
      "duplicate responses": /Multiple generator responses/,
      "oversized response": /Generator response exceeds/,
      "oversized stderr": /Generator stderr exceeds/,
      deadline: /Generator deadline exceeded/,
    }[name];
    await assert.rejects(
      g.call(request),
      (e) => e.code === "EC_MUTATION_PROTOCOL" && message.test(e.message),
    );
    await stopped(g);
  });
test("closing during a request rejects it and reaps the child", async (t) => {
  const g = new Generator(
    fake(t, `process.stdin.resume();setInterval(()=>{},1000);`),
  );
  const pending = assert.rejects(
    g.call(request),
    (e) => e.code === "EC_MUTATION_PROTOCOL",
  );
  await stopped(g);
  await pending;
});
test("sequential contract rejects overlap without corrupting an in-flight request", async () => {
  const g = new Generator(binary);
  try {
    const first = g.call(request);
    await assert.rejects(g.call(request), /sequential/);
    await first;
    await g.call(request);
  } finally {
    await stopped(g);
  }
});
test("server rejects malformed, truncated and duplicate frames", () => {
  for (const input of [
    "{bad}\n",
    JSON.stringify({ version: 1, id: 1, request }),
    [1, 1].map((id) => JSON.stringify({ version: 1, id, request })).join("\n") +
      "\n",
  ]) {
    const r = spawnSync(binary, ["__mutation-source-lines"], {
      input,
      encoding: "utf8",
      timeout: 3000,
    });
    assert.equal(r.status, 2);
    assert.match(r.stderr, /Invalid|truncated/);
  }
});
test("repeated AST allocation reaches a bounded RSS plateau", async () => {
  const g = new Generator(binary),
    samples = [];
  const source = Array.from(
    { length: 100 },
    (_, i) => `export function f${i}(x:number){return x+1;}`,
  ).join("\n");
  try {
    for (let n = 1; n <= 500; n++) {
      await g.call({ file: "memory.ts", source, facts: true });
      if ([50, 100, 250, 500].includes(n)) {
        const r = spawnSync("ps", ["-p", String(g.child.pid), "-o", "rss="], {
          encoding: "utf8",
        });
        assert.equal(r.status, 0, r.stderr);
        samples.push({ requests: n, rssKiB: Number(r.stdout.trim()) });
      }
    }
    const growth = samples.at(-1).rssKiB - samples[0].rssKiB;
    assert(growth < 32 * 1024, JSON.stringify(samples));
  } finally {
    await stopped(g);
  }
});

test("engine fingerprint mismatch is an input error and malformed source requests are protocol errors", async () => {
  const g=new Generator(binary);
  try {
    await assert.rejects(g.call({...request,engineFingerprint:"0".repeat(64)}),{code:"EC_MUTATION_INPUT"});
    assert((await g.call(request)).candidates.length>0);
    await assert.rejects(g.call({file:"x.ts",source:3}),{code:"EC_MUTATION_PROTOCOL"});
  } finally {await stopped(g);}
});
