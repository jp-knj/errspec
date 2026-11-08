import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const rustc = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
const target = rustc.stdout?.match(/^host: (.+)$/m)?.[1];
if (!target) throw new Error("Cannot determine Rust build target");
const result = spawnSync(
  "cargo",
  [
    "metadata",
    "--locked",
    "--format-version",
    "1",
    "--filter-platform",
    target,
  ],
  { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
);
if (result.status !== 0)
  throw new Error(result.stderr || "cargo metadata failed");
const metadata = JSON.parse(result.stdout);
const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
const included = new Set();
function visit(id) {
  if (included.has(id)) return;
  included.add(id);
  for (const d of nodes.get(id)?.deps || []) visit(d.pkg);
}
visit(metadata.resolve.root);
for (const pkg of metadata.packages
  .filter((p) => p.source && included.has(p.id))
  .sort((a, b) => a.name.localeCompare(b.name))) {
  let dir = dirname(pkg.manifest_path);
  const files = [];
  // These published crates omit the license text. Vendor the exact upstream revisions:
  // https://github.com/oxc-project/oxc/blob/f799f174b5db10f1fa9e136594e53de82d4f4f83/LICENSE
  // https://github.com/oxc-project/oxc-index-vec/blob/8e09fe324eb6df02f56e4eacdfac958930300380/LICENSE
  let supplied;
  if (pkg.name.startsWith("oxc_") && pkg.version === "0.157.0" && pkg.repository === "https://github.com/oxc-project/oxc")
    supplied = "oxc-0.157.0-LICENSE.txt";
  if (pkg.name === "oxc_index" && pkg.version === "5.0.0" && pkg.repository === "https://github.com/oxc-project/oxc-index-vec")
    supplied = "oxc-index-5.0.0-LICENSE.txt";
  if (supplied) files.push(fileURLToPath(new URL(`../docs/engineering/licenses/${supplied}`, import.meta.url)));
  if (pkg.license_file) files.push(join(dir, pkg.license_file));
  for (let depth = 0; depth < 5 && !files.length; depth++) {
    files.push(
      ...readdirSync(dir)
        .filter((name) =>
          /^(LICENSE|LICENCE|COPYING|NOTICE)([.-]|$)/i.test(name),
        )
        .map((name) => join(dir, name)),
    );
    dir = dirname(dir);
  }
  if (!files.length)
    throw new Error(
      `Missing license text for ${pkg.name}; supply it before distributing the binary`,
    );
  process.stdout.write(
    `\n===== ${pkg.name} ${pkg.version} (${pkg.license || "see text"}) =====\n`,
  );
  for (const file of [...new Set(files)]) {
    if (!existsSync(file)) throw new Error(`License file missing: ${file}`);
    process.stdout.write(readFileSync(file, "utf8") + "\n");
  }
}
process.stdout.write("\n"+readFileSync(fileURLToPath(new URL("../packages/mutation-driver/THIRD_PARTY_NOTICES.txt",import.meta.url)),"utf8"));
