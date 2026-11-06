import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { getEventListeners } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { formatReport, runMutation, selectTests } from "../packages/core/index.mjs";
import { exchangeNative } from "../packages/core/native-transport.mjs";

const repo = fileURLToPath(new URL("../", import.meta.url));
const binary = process.env.ERRSPEC_BIN || join(repo, "target/debug/errspec");
const mib = 1024 * 1024;

function temporary(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "errspec-core-transport-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function probe(t, body, { early = "" } = {}) {
  const cwd = temporary(t), path = join(cwd, "native.mjs");
  writeFileSync(path, `#!${process.execPath}\n${early}\nlet input="";for await(const chunk of process.stdin)input+=chunk;const request=JSON.parse(input);\nconst mutation=(code=0,extra={})=>({protocol:1,exitCode:code,report:{schemaVersion:1,kind:request.operation==="selectTests"?"errspec/test-selection":"errspec/mutation",...extra}});\nconst reply=(value,code=0)=>{process.exitCode=code;process.stdout.write(JSON.stringify(value));};\n${body}\n`, { mode: 0o755 });
  return { binary: path, cwd };
}

async function ready(path) {
  const deadline = Date.now() + 5000;
  while (!existsSync(path) && Date.now() < deadline) await delay(10);
  assert.ok(existsSync(path), "Native probe did not start");
}

test("native request routing preserves cwd, config and completed nonzero mutation reports", async t => {
  const transport = probe(t, `const code=request.options.code||0;reply(mutation(code,{received:{command:process.argv[2],cwd:process.cwd(),request}}),code);`);
  for (const code of [0, 1, 2, 130, 143]) {
    const result = await runMutation({ code }, { ...transport, configFile: "settings.json" });
    assert.equal(result.exitCode, code);
    assert.deepEqual(result.report.received, {
      command: "__mutation-api", cwd: transport.cwd,
      request: { protocol: 1, operation: "runMutation", options: { code }, configFile: "settings.json" },
    });
  }
  const selection = await selectTests({ since: "HEAD" }, transport);
  assert.equal(selection.report.kind, "errspec/test-selection");
  assert.equal(selection.report.received.request.operation, "selectTests");
  const formatted = await formatReport({ marker: "report" }, { reporter: "agent", root: "project", sources: { "a.ts": "original" } }, {
    ...probe(t, `reply({protocol:1,output:JSON.stringify({command:process.argv[2],request,cwd:process.cwd()})});`),
  });
  const received = JSON.parse(formatted);
  assert.equal(received.command, "__format-report");
  assert.deepEqual(received.request, { protocol: 1, report: { marker: "report" }, reporter: "agent", root: "project", sources: { "a.ts": "original" } });
});

test("each API retains its own response and exit-code validation", async t => {
  for (const response of [
    { protocol: 2, exitCode: 0, report: { kind: "errspec/mutation", schemaVersion: 1 } },
    { protocol: 1, exitCode: 1, report: { kind: "errspec/mutation", schemaVersion: 1 } },
    { protocol: 1, exitCode: 0, report: { kind: "errspec/test-selection", schemaVersion: 1 } },
    { protocol: 1, exitCode: 0, report: { kind: "errspec/mutation", schemaVersion: 2 } },
  ]) await assert.rejects(runMutation({}, probe(t, `reply(${JSON.stringify(response)});`)), /Invalid mutation API response/);
  await assert.rejects(formatReport({}, {}, probe(t, `reply({protocol:2,error:"native failure"},2);`)), /Invalid formatter response/);
  await assert.rejects(formatReport({}, {}, probe(t, `reply({protocol:1,error:"native failure"},2);`)), /native failure/);
  await assert.rejects(formatReport({}, {}, probe(t, `reply({protocol:1},2);`)), /Report formatting failed/);
  await assert.rejects(formatReport({}, {}, probe(t, `reply({protocol:1,output:42});`)), /Invalid formatter response/);
  for (const invoke of [transport => runMutation({}, transport), transport => formatReport({}, {}, transport)]) {
    await assert.rejects(invoke(probe(t, `process.stdout.write("{malformed");`)), SyntaxError);
    await assert.rejects(invoke({ binary: join(temporary(t), "missing") }), { code: "ENOENT" });
  }
  await assert.rejects(runMutation({}, probe(t, `process.kill(process.pid,"SIGTERM");`)), /Mutation API terminated by SIGTERM/);
  await assert.rejects(formatReport({}, {}, probe(t, `process.kill(process.pid,"SIGTERM");`)), /Formatter terminated by SIGTERM/);
});

test("invalid JSON values and oversized format requests fail before starting native execution", async t => {
  const transport = probe(t, `reply(mutation());`, { early: `import {writeFileSync} from "node:fs";writeFileSync(new URL("./started",import.meta.url),"yes");` });
  for (const value of [() => {}, Symbol("invalid"), 1n, NaN, Infinity]) {
    await assert.rejects(runMutation({ nested: { value } }, transport), /Mutation options must contain JSON values only/);
    await assert.rejects(formatReport({ nested: { value } }, {}, transport), /Report options must contain JSON values only/);
  }
  await assert.rejects(formatReport({ value: "x".repeat(64 * mib) }, {}, transport), /Report request exceeds 64 MiB/);
  assert.equal(existsSync(join(transport.cwd, "started")), false);
});

test("a native response survives an early stdin close", async t => {
  // Native validation can finish before a large request has drained into stdin.
  const transport = probe(t, "", { early: `process.stdout.write(JSON.stringify({protocol:1,exitCode:2,report:{kind:"errspec/mutation",schemaVersion:1}}),()=>process.exit(2));` });
  const result = await runMutation({ padding: "x".repeat(mib) }, transport);
  assert.equal(result.exitCode, 2);
  const formatter = probe(t, "", { early: `process.stdout.write(JSON.stringify({protocol:1,error:"native rejected request"}),()=>process.exit(2));` });
  await assert.rejects(formatReport({ padding: "x".repeat(mib) }, {}, formatter), /native rejected request/);
});

test("mutation cancellation returns its final report and removes its signal listener", async t => {
  const transport = probe(t, `const timer=setInterval(()=>{},1000);process.on("SIGINT",()=>{clearInterval(timer);reply(mutation(130),130);});writeFileSync("ready","yes");`, { early: `import {writeFileSync} from "node:fs";` });
  const reason = new Error("cancelled before start");
  await assert.rejects(runMutation({}, { ...transport, signal: AbortSignal.abort(reason) }), error => error === reason);
  assert.equal(existsSync(join(transport.cwd, "ready")), false);
  const controller = new AbortController(), pending = runMutation({}, { ...transport, signal: controller.signal });
  await ready(join(transport.cwd, "ready"));
  controller.abort();
  assert.equal((await pending).exitCode, 130);
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  // Formatting's public transport is binary/cwd only; preserve its existing behavior.
  const formatted = await formatReport({}, {}, { ...probe(t, `reply({protocol:1,output:"formatted"});`), signal: AbortSignal.abort() });
  assert.equal(formatted, "formatted");
});

test("mutation cancellation escalates when native ignores SIGINT", { timeout: 12000 }, async t => {
  const transport = probe(t, `process.on("SIGINT",()=>writeFileSync("interrupted","yes"));setInterval(()=>{},1000);setTimeout(()=>process.exit(99),10000).unref();writeFileSync("ready","yes");`, { early: `import {writeFileSync} from "node:fs";` });
  const controller = new AbortController(), pending = runMutation({}, { ...transport, signal: controller.signal });
  await ready(join(transport.cwd, "ready"));
  controller.abort();
  await assert.rejects(pending, /Mutation API terminated by SIGKILL/);
  assert.equal(readFileSync(join(transport.cwd, "interrupted"), "utf8"), "yes");
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
});

test("mutation and formatter retain their distinct stdout limits", { timeout: 180000 }, async t => {
  const transport = probe(t, `const bytes=request.options?.bytes||request.report.bytes;reply(process.argv[2]==="__mutation-api"?mutation(0,{padding:"x".repeat(bytes)}):{protocol:1,output:"x".repeat(bytes)});`);
  await assert.rejects(runMutation({ bytes: 65 * mib }, transport), /Mutation API output exceeds 64 MiB/);
  assert.equal((await formatReport({ bytes: 65 * mib }, {}, transport)).length, 65 * mib);
  await assert.rejects(formatReport({ bytes: 129 * mib }, {}, transport), /Formatter output exceeds 128 MiB/);
});

test("stdout limits count decoded UTF-8 bytes across split and malformed sequences", async t => {
  const stream = fragments => probe(t, `for(const bytes of ${JSON.stringify(fragments.map(fragment => [...fragment]))}){await new Promise(resolve=>process.stdout.write(Buffer.from(bytes),resolve));await new Promise(resolve=>setTimeout(resolve,10));}`);
  const receive = (transport, outputLimit) => exchangeNative({
    ...transport, command: "__utf8-probe", input: "{}", label: "UTF-8 probe",
    outputLimit, outputError: "Decoded output exceeded its limit",
  });
  const encoded = Buffer.from('{"value":"€😀"}');
  const euro = encoded.indexOf(Buffer.from("€")), emoji = encoded.indexOf(Buffer.from("😀"));
  const valid = stream([encoded.subarray(0, euro + 1), encoded.subarray(euro + 1, emoji + 2), encoded.subarray(emoji + 2)]);
  assert.deepEqual((await receive(valid, encoded.length)).response, { value: "€😀" });
  await assert.rejects(receive(valid, encoded.length - 1), /Decoded output exceeded its limit/);
  const fragments = [Buffer.from('{"value":"'), Buffer.from([0xe2]), Buffer.from('"}')];
  const raw = Buffer.concat(fragments), decodedBytes = Buffer.byteLength(raw.toString("utf8"));
  assert.ok(decodedBytes > raw.length, "Malformed UTF-8 expands to a replacement character");
  const malformed = stream(fragments);
  assert.deepEqual((await receive(malformed, decodedBytes)).response, { value: "�" });
  await assert.rejects(receive(malformed, raw.length), /Decoded output exceeded its limit/);
});

test("packed core includes its private transport and resolves the installed native launcher", t => {
  const root = temporary(t), core = join(root, "node_modules/@errspec/core"), cli = join(root, "node_modules/@errspec/cli");
  mkdirSync(core, { recursive: true }); mkdirSync(cli, { recursive: true });
  const packed = spawnSync("npm", ["pack", "--offline", "--ignore-scripts", "--json", "--pack-destination", root, "--cache", join(root, "cache")], {
    cwd: join(repo, "packages/core"), encoding: "utf8", timeout: 30000,
  });
  assert.equal(packed.status, 0, packed.stderr + packed.stdout);
  const [metadata] = JSON.parse(packed.stdout);
  assert.ok(metadata.files.some(file => file.path === "native-transport.mjs"));
  const tar = spawnSync("tar", ["-xzf", join(root, metadata.filename), "-C", core, "--strip-components=1"], { encoding: "utf8" });
  assert.equal(tar.status, 0, tar.stderr);
  const native = probe(t, `reply(process.argv[2]==="__mutation-api"?mutation():{protocol:1,output:"installed formatter"});`);
  writeFileSync(join(cli, "package.json"), JSON.stringify({ name: "@errspec/cli", type: "module", exports: { "./native": "./native.mjs" } }));
  writeFileSync(join(cli, "native.mjs"), `export function nativeBinary(){return ${JSON.stringify(native.binary)}}`);
  writeFileSync(join(root, "consumer.mjs"), `import assert from "node:assert/strict";import {runMutation,formatReport} from "@errspec/core";assert.equal((await runMutation()).exitCode,0);assert.equal(await formatReport({}),"installed formatter");`);
  const consumer = spawnSync(process.execPath, [join(root, "consumer.mjs")], { cwd: root, encoding: "utf8", timeout: 15000 });
  assert.equal(consumer.status, 0, consumer.stderr + consumer.stdout);
});

test("real native mutation, quality/configuration failures, selection and formatting preserve their reports", { timeout: 60000 }, async t => {
  const root = temporary(t), source = "export const value=()=>true;\n";
  mkdirSync(join(root, "src")); mkdirSync(join(root, "node_modules"));
  symlinkSync(join(repo, "examples/checkout/node_modules/vitest"), join(root, "node_modules/vitest"), "dir");
  writeFileSync(join(root, "package.json"), JSON.stringify({ type: "module" }));
  writeFileSync(join(root, "src/value.ts"), source);
  writeFileSync(join(root, "value.test.ts"), 'import {test,expect} from "vitest";import {value} from "./src/value";test("observes",()=>expect(value()).toBeDefined());');
  writeFileSync(join(root, "vitest.config.mjs"), 'export default {test:{include:["value.test.ts"]}};');
  const transport = { binary, cwd: root }, options = { node: process.execPath };
  const result = await runMutation(options, transport);
  assert.equal(result.exitCode, 0, JSON.stringify(result));
  assert.equal(result.report.runComplete, true);
  assert.ok(result.report.summary.survived > 0);
  const mte = JSON.parse(await formatReport(result.report, { reporter: "json" }, transport));
  assert.equal(mte.framework.name, "errspec");
  assert.equal(mte.files["src/value.ts"].source, source);
  assert.equal((await runMutation({ ...options, failOnUndetected: true }, transport)).exitCode, 1);
  const invalid = await runMutation({ unknown: true }, transport);
  assert.equal(invalid.exitCode, 2); assert.equal(invalid.report.testsExecuted, false);
  for (const args of [["init", "-q"], ["add", "."], ["-c", "commit.gpgSign=false", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "base"]]) {
    const git = spawnSync("git", args, { cwd: root, encoding: "utf8" });
    assert.equal(git.status, 0, git.stderr);
  }
  writeFileSync(join(root, "src/value.ts"), source.replace("=>", "=>/* changed */"));
  const selection = await selectTests({ ...options, since: "HEAD" }, transport);
  assert.equal(selection.exitCode, 0, JSON.stringify(selection));
  assert.equal(selection.report.runComplete, true);
  assert.deepEqual(selection.report.files, ["value.test.ts"]);
});
