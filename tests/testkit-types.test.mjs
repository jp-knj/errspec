import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { copyFileSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../", import.meta.url));
const ts = createRequire(new URL("../packages/analysis-driver/package.json", import.meta.url))("typescript6");
const nodeTypes = realpathSync(join(repo, "tests/fixtures/runners/node_modules/@types"));

test("generated declarations and a standalone vendored source retain strict public types", t => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "errspec-testkit-types-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const pkg = join(root, "node_modules/@errspec/testkit"), vendor = join(root, "vendor");
  mkdirSync(pkg, { recursive: true });
  mkdirSync(vendor);
  for (const name of ["package.json", "index.mjs", "index.d.mts"]) copyFileSync(join(repo, "packages/testkit", name), join(pkg, name));
  // A copied TypeScript file must not require any other testkit source file.
  copyFileSync(join(repo, "packages/testkit/index.ts"), join(vendor, "testkit.ts"));
  writeFileSync(join(root, "package.json"), '{"type":"module"}\n');
  const consumer = join(root, "consumer.mts");
  writeFileSync(consumer, `
import * as shipped from "@errspec/testkit";
import * as vendored from "./vendor/testkit.ts";
const scenario: typeof shipped.scenarioTest = vendored.scenarioTest;
const reverseScenario: typeof vendored.scenarioTest = shipped.scenarioTest;
const atomicity: typeof shipped.checkFailureAtomicity = vendored.checkFailureAtomicity;
const reverseAtomicity: typeof vendored.checkFailureAtomicity = shipped.checkFailureAtomicity;
declare const scope: shipped.ScenarioScope;
declare const sourceScope: vendored.ScenarioScope;
const request = scope.fault("network", async (id: number, label: string) => ({ id, label }));
const response: Promise<{id: number; label: string}> = request(1, "first");
// @ts-expect-error wrapped dependency arguments stay typed
request("wrong", 1);
// @ts-expect-error expected must not widen the actual comparison type
scope.check("status", 200, "200");
// @ts-expect-error standalone source keeps the same NoInfer constraint
sourceScope.check("status", 200, "200");
const definition: shipped.ScenarioDefinition = {contract: "payment", id: "offline", tag: "Offline"};
// @ts-expect-error definition identity remains readonly
definition.id = "changed";
const report: Promise<shipped.FailureAtomicityReport<{ok: boolean}, {orders: number}>> = atomicity<{ok: boolean}, {orders: number}>({
  setup: () => ({
    snapshot: () => ({orders: 0}),
    run: async failure => failure.wrap("save", (count: number) => ({ok: count > 0}))(1),
  }),
  faults: [{id: "offline", dependency: "save", inject: () => ({ok: false})}],
  isFailure: result => !result.ok,
});
declare const failure: shipped.FailureScope;
const wrapped: (count: number) => Promise<boolean> = failure.wrap("save", async (count: number) => count > 0);
void [scenario, reverseScenario, reverseAtomicity, response, report, wrapped];
`);
  const program = ts.createProgram([consumer], {
    strict: true, noEmit: true, skipLibCheck: false, allowImportingTsExtensions: true,
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.NodeNext, moduleResolution: ts.ModuleResolutionKind.NodeNext,
    types: ["node"], typeRoots: [nodeTypes],
  });
  const diagnostics = ts.getPreEmitDiagnostics(program).map(diagnostic => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"));
  assert.deepEqual(diagnostics, []);
});
