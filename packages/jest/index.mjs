import {runMutation as execute} from "@errspec/core";
export {defineConfig} from "@errspec/core";
/** Uses the same validated request and native planning/verdict pipeline as core. */
export async function runMutation(options={},transport={}){
  if(options.runner!==undefined&&options.runner!=="jest")throw new TypeError("@errspec/jest fixes runner to jest");
  return execute({...options,runner:"jest"},transport);
}
