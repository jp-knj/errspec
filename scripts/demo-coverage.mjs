// End-to-end check of `errspec coverage` against real Vitest. Only mutates a temporary copy.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const project = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const example = join(project, "examples/checkout");
const binary = resolve(process.argv[2] ?? join(project, "target/debug/errspec"));
assert(existsSync(binary), "Run cargo build first");
assert(existsSync(join(example, "node_modules/.bin/vitest")), "Run npm ci in examples/checkout first");
const work = mkdtempSync(join(tmpdir(), "errspec-coverage-"));

function coverage(label, extra, status) {
  const result = spawnSync(binary, ["coverage", "--format", "json", ...extra], { cwd: work, encoding: "utf8", timeout: 120_000 });
  assert.equal(result.error, undefined, String(result.error));
  assert.equal(result.status, status, `${label}: ${result.stderr}\n${result.stdout}`);
  const report = JSON.parse(result.stdout);
  console.log(`${label}: exit ${result.status}, ${JSON.stringify(report.summary)}`);
  return report;
}

// Status of each tag of the public payment error type.
function tags(report) {
  const type = report.types.find(t => t.source.export === "GatewayError");
  assert(type, JSON.stringify(report.types));
  return Object.fromEntries(type.tags.map(t => [t.tag, t.status]));
}

function edit(file, from, to) {
  const path = join(work, file);
  const text = readFileSync(path, "utf8");
  assert(text.includes(from), `${file} lacks ${from}`);
  writeFileSync(path, text.replace(from, to));
}

try {
  cpSync(example, work, { recursive: true, filter: p => !p.includes("node_modules") });
  symlinkSync(join(example, "node_modules"), join(work, "node_modules"), "dir");

  // The example tests inject each failure through a stub and assert only the HTTP status.
  const unchanged = coverage("existing tests", ["--fail-on-missing"], 0);
  assert.equal(unchanged.runnerOk, true);
  assert.deepEqual(tags(unchanged), { Declined: "mentioned", RateLimited: "mentioned" });

  // A new failure that the code handles but no test exercises.
  edit("src/gateway.ts", 'GatewayFailure<"RateLimited">', 'GatewayFailure<"RateLimited"> | GatewayFailure<"Timeout">');
  edit("src/checkout.ts", 'case "RateLimited": return { status: 429 };', 'case "RateLimited": return { status: 429 };\n      case "Timeout": return { status: 504 };');
  assert.equal(tags(coverage("new variant", [], 0)).Timeout, "missing");
  coverage("new variant, strict", ["--fail-on-missing"], 1);

  // An ordinary Vitest test that asserts the failure value is enough; no errspec API is used.
  writeFileSync(join(work, "tests/timeout.test.ts"), `
import { expect, it, vi } from "vitest";
import { checkout } from "../src/checkout";

it("Timeout returns 504 without creating an order", async () => {
  const charge = vi.fn(async () => ({ ok: false as const, error: { _tag: "Timeout" as const, retryAfter: 5 } }));
  const createOrder = vi.fn(async () => {});
  expect(await checkout({ charge, createOrder })).toEqual({ status: 504 });
  await expect(charge.mock.results[0]!.value).resolves.toMatchObject({ error: { _tag: "Timeout" } });
  expect(createOrder).not.toHaveBeenCalled();
});
`);
  const covered = coverage("asserted by a plain test", ["--fail-on-missing"], 0);
  assert.equal(tags(covered).Timeout, "asserted");
  console.log(JSON.stringify({ acceptance: "passed" }));
} finally {
  rmSync(work, { recursive: true, force: true });
}
