// Stage publishable tarballs without registry access or lifecycle scripts.
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
  chmodSync,
  rmSync,
} from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { targets } from "../packages/cli/platforms.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [target, binaryArg, outputArg = "artifacts/npm"] = process.argv.slice(2);
if (!targets[target] || !binaryArg)
  throw new Error(
    "Usage: node scripts/pack.mjs <linux-x64|darwin-arm64|darwin-x64> <binary> [output-dir]",
  );
const binary = resolve(binaryArg);
const out = resolve(outputArg);
const cli = JSON.parse(
  readFileSync(join(root, "packages/cli/package.json"), "utf8"),
);
const kit = JSON.parse(
  readFileSync(join(root, "packages/testkit/package.json"), "utf8"),
);
const core = JSON.parse(readFileSync(join(root, "packages/core/package.json"), "utf8"));
if (core.version !== cli.version || core.dependencies["@errspec/cli"] !== cli.version)
  throw new Error("Core and CLI versions must match");
const cargo = readFileSync(join(root, "Cargo.toml"), "utf8").match(
  /^version = "([^"]+)"/m,
)?.[1];
if (cli.version !== kit.version || cli.version !== cargo)
  throw new Error("CLI, testkit and Cargo versions must match");
for (const key of Object.keys(targets)) {
  if (cli.optionalDependencies[`@errspec/cli-${key}`] !== cli.version)
    throw new Error(`Native dependency version mismatch: ${key}`);
}
mkdirSync(out, { recursive: true });
const stage = mkdtempSync(join(tmpdir(), "errspec-pack-"));
function pack(dir) {
  const result = spawnSync(
    "npm",
    ["pack", "--ignore-scripts", "--json", "--pack-destination", out],
    { cwd: dir, encoding: "utf8" },
  );
  if (result.status !== 0)
    throw new Error(result.stderr || result.error?.message);
  const entry = JSON.parse(result.stdout)[0];
  const bytes = readFileSync(join(out, entry.filename));
  return {
    name: entry.name,
    version: entry.version,
    filename: entry.filename,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    bytes: bytes.length,
  };
}
try {
  const native = join(stage, "native");
  mkdirSync(join(native, "bin"), { recursive: true });
  cpSync(binary, join(native, "bin/errspec"));
  chmodSync(join(native, "bin/errspec"), 0o755);
  const info = targets[target];
  writeFileSync(
    join(native, "package.json"),
    JSON.stringify(
      {
        name: `@errspec/cli-${target}`,
        version: cli.version,
        description: `errspec native binary for ${target}`,
        license: "MIT",
        repository: cli.repository,
        os: [info.os],
        cpu: [info.cpu],
        ...(info.libc ? { libc: info.libc } : {}),
        files: ["bin", "LICENSE", "THIRD_PARTY_NOTICES.txt"],
        publishConfig: { access: "public" },
      },
      null,
      2,
    ) + "\n",
  );
  cpSync(join(root, "LICENSE"), join(native, "LICENSE"));
  const notices = spawnSync(
    process.execPath,
    [join(root, "scripts/notices.mjs")],
    { cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
  );
  if (notices.status !== 0)
    throw new Error(
      notices.stderr ||
        notices.error?.message ||
        "Cannot gather dependency licenses",
    );
  writeFileSync(join(native, "THIRD_PARTY_NOTICES.txt"), notices.stdout);
  const packages = [pack(native)];
  for (const name of ["cli", "testkit", "core"]) {
    const dest = join(stage, name);
    cpSync(join(root, "packages", name), dest, { recursive: true });
    cpSync(join(root, "LICENSE"), join(dest, "LICENSE"));
    if (name === "cli") chmodSync(join(dest, "bin/errspec.mjs"), 0o755);
    packages.push(pack(dest));
  }
  writeFileSync(
    join(out, `manifest-${target}.json`),
    JSON.stringify(
      { target, rustTarget: info.triple, version: cli.version, packages },
      null,
      2,
    ) + "\n",
  );
  console.log(JSON.stringify({ target, packages }, null, 2));
} finally {
  rmSync(stage, { recursive: true, force: true });
}
