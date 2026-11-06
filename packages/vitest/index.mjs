import {runMutation as execute} from "@errspec/core";
export {defineConfig} from "@errspec/core";
/** Uses the same validated request and native planning/verdict pipeline as core. */
export async function runMutation(options={},transport={}){
  if(options.runner!==undefined&&options.runner!=="vitest")throw new TypeError("@errspec/vitest fixes runner to vitest");
  return execute({...options,runner:"vitest"},transport);
}
