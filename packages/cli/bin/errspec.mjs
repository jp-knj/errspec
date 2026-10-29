#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { accessSync, constants } from "node:fs";
import { platformPackage } from "../platforms.mjs";

const require = createRequire(import.meta.url);
let binary;
try {
  const pkg = platformPackage(
    process.platform,
    process.arch,
    process.platform === "linux"
      ? process.report.getReport().header.glibcVersionRuntime
      : undefined,
  );
  binary = require.resolve(`${pkg}/bin/errspec`);
  accessSync(binary, constants.X_OK);
} catch (error) {
  console.error(
    `errspec: ${error.message}\nInstall @errspec/cli with optional dependencies enabled. No Rust build is performed during installation.`,
  );
  process.exit(2);
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
const signals = new Map(
  ["SIGINT", "SIGTERM"].map((signal) => [signal, () => child.kill(signal)]),
);
for (const [signal, handler] of signals) process.on(signal, handler);
child.once("error", (error) => {
  console.error(`errspec: cannot start native binary: ${error.message}`);
  process.exitCode = 2;
});
child.once("close", (code, signal) => {
  for (const [name, handler] of signals) process.off(name, handler);
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 2;
});
