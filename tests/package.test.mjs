import { spawnSync } from "node:child_process";
import test from "node:test";
import assert from "node:assert/strict";
import { platformPackage } from "../packages/cli/platforms.mjs";
test("selects only compatible native packages", () => {
  assert.equal(
    platformPackage("linux", "x64", "2.35"),
    "@errspec/cli-linux-x64",
  );
  assert.equal(platformPackage("darwin", "arm64"), "@errspec/cli-darwin-arm64");
  assert.equal(platformPackage("darwin", "x64"), "@errspec/cli-darwin-x64");
  assert.throws(() => platformPackage("linux", "x64"), /musl/);
  assert.throws(() => platformPackage("linux", "x64", "2.34"), /2.35/);
  assert.throws(() => platformPackage("win32", "x64"), /Unsupported/);
  assert.throws(() => platformPackage("linux", "arm64", "2.40"), /Unsupported/);
});

test("native notices include the licenses omitted from the published Oxc crates", () => {
  const result = spawnSync(process.execPath, [new URL("../scripts/notices.mjs", import.meta.url).pathname], {
    encoding: "utf8", maxBuffer: 32 * 1024 * 1024,
  });
  assert.equal(result.status, 0, result.stderr);
  for (const name of ["allocator", "ast", "ast_visit", "parser", "semantic", "span", "syntax"])
    assert(result.stdout.includes(`===== oxc_${name} 0.157.0 (MIT) =====`));
  assert(result.stdout.includes("===== oxc_index 5.0.0 (MIT) ====="));
  assert(result.stdout.includes("Copyright (c) 2024-present VoidZero Inc. & Contributors"));
  assert(result.stdout.includes("Permission is hereby granted"));
});
