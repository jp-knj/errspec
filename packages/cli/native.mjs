import { createRequire } from "node:module";
import { accessSync, constants } from "node:fs";
import { platformPackage } from "./platforms.mjs";
const require = createRequire(import.meta.url);
export function nativeBinary() {
  const pkg=platformPackage(process.platform,process.arch,process.platform === "linux" ? process.report.getReport().header.glibcVersionRuntime : undefined);
  const binary=require.resolve(`${pkg}/bin/errspec`);
  accessSync(binary,constants.X_OK);
  return binary;
}
