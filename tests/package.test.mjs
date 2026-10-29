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
