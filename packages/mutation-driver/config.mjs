// Imported only when a script configuration was selected. User output stays on stderr.
import { pathToFileURL } from "node:url";
const output = process.stdout.write.bind(process.stdout);
process.stdout.write = (...args) => process.stderr.write(...args);
try {
  const { default: config } = await import(pathToFileURL(process.argv[2]).href);
  const check = (value, seen = new Set()) => {
    if (value === null || typeof value === "string" || typeof value === "boolean") return;
    if (typeof value === "number" && Number.isFinite(value)) return;
    if (typeof value !== "object" || seen.has(value)) throw new Error("Configuration must contain JSON values only");
    if (!Array.isArray(value) && Object.getPrototypeOf(value) !== Object.prototype) throw new Error("Configuration must contain plain objects");
    const next = new Set(seen).add(value);
    for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
      if (descriptor.enumerable) {
        if (!("value" in descriptor)) throw new Error("Configuration getters are not supported");
        check(descriptor.value, next);
      }
    }
  };
  if (!config || Array.isArray(config) || typeof config !== "object") throw new Error("Export a default configuration object");
  check(config);
  output(JSON.stringify(config));
} catch (error) {
  console.error(error.stack || error);
  process.exitCode = 2;
}
