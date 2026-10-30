import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { format } from "node:util";
import { openCompiler, syntax, tags, TagError } from "./compiler.mjs";
import { scan } from "./scan.mjs";
import { scenarios } from "./scenarios.mjs";

const output = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, encoding, callback) =>
  process.stderr.write(chunk, encoding, callback);
console.log = (...args) => process.stderr.write(format(...args) + "\n");
console.info = console.log;
let compiler;
try {
  const request = JSON.parse(readFileSync(0, "utf8"));
  if (request.protocol !== 1) throw new Error("Unsupported analysis protocol");
  compiler = await openCompiler(request.package, request.project);
  const result = {
    protocol: 1,
    engine: "typescript",
    version: compiler.version,
    diagnostics: compiler.diagnostics,
    tags: [],
    facts: null,
    scenarios: [],
  };
  if (compiler.noCheck)
    result.diagnostics.push({
      code: "EC_CONFIG",
      message: "noCheck must be false for errspec analysis",
    });
  if (!result.diagnostics.length) {
    const syn = syntax(compiler, request.root);
    for (const [id, source] of request.sources.entries()) {
      try {
        result.tags.push({
          id,
          tags: tags(compiler, {
            ...source,
            file: resolve(request.root, source.file),
          }),
          error: null,
        });
      } catch (error) {
        if (!(error instanceof TagError)) throw error;
        result.tags.push({ id, tags: [], error: error.message });
      }
    }
    if (request.discriminants)
      result.facts = scan(
        compiler,
        syn,
        request.root,
        request.discriminants,
        (source) => tags(compiler, source),
      );
    result.scenarios = scenarios(compiler, syn, request.scenarioFiles);
    syn.verify();
  }
  output(JSON.stringify(result) + "\n");
} catch (error) {
  process.stderr.write(String(error.stack || error) + "\n");
  process.exitCode = 2;
} finally {
  compiler?.close();
}
