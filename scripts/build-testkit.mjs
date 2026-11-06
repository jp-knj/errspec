import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";

// Reuse the pinned development compiler; consumers need no build or dependency.
const require = createRequire(new URL("../packages/analysis-driver/package.json", import.meta.url));
const ts = require("typescript6");
if (ts.version !== "6.0.3") throw new Error("Build the testkit with TypeScript 6.0.3");
if (process.argv.slice(2).some(argument => argument !== "--check")) {
  throw new Error("Usage: node scripts/build-testkit.mjs [--check]");
}

const source = readFileSync(new URL("../packages/testkit/index.ts", import.meta.url), "utf8");
const options = {
  fileName: "index.ts",
  reportDiagnostics: true,
  compilerOptions: {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    newLine: ts.NewLineKind.LineFeed,
  },
};
const generated = [
  ["index.mjs", ts.transpileModule(source, options)],
  ["index.d.mts", ts.transpileDeclaration(source, options)],
].map(([name, result]) => {
  const errors = result.diagnostics?.filter(diagnostic => diagnostic.category === ts.DiagnosticCategory.Error) || [];
  if (errors.length) {
    throw new Error(errors.map(diagnostic => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")).join("\n"));
  }
  return [new URL(`../packages/testkit/${name}`, import.meta.url),
    `// Generated from index.ts by scripts/build-testkit.mjs. Do not edit.\n${result.outputText.trimEnd()}\n`];
});

if (process.argv.includes("--check")) {
  const stale = generated.filter(([path, text]) => {
    try { return readFileSync(path, "utf8") !== text; }
    catch (error) { if (error.code === "ENOENT") return true; throw error; }
  });
  if (stale.length) {
    throw new Error(`Rebuild committed testkit artifacts: ${stale.map(([path]) => path.pathname.split("/").pop()).join(", ")}; run node scripts/build-testkit.mjs`);
  }
} else {
  for (const [path, text] of generated) writeFileSync(path, text);
}
