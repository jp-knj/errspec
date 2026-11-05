import { existsSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
const extensions=["ts","mts","cts","js","mjs","cjs"];
const configs=(root,names)=>names.some(name=>[...extensions,"json"].some(ext=>existsSync(join(root,`${name}.config.${ext}`))));
export function resolvePackage(root,name,explicit) {
  if(explicit) {
    if(!existsSync(join(explicit,"package.json")))throw new Error(`Runner package not found: ${explicit}`);
    return realpathSync(explicit);
  }
  try{return dirname(realpathSync(createRequire(join(root,"package.json")).resolve(`${name}/package.json`)));}catch{return undefined;}
}
export function detectRunner(root,options={}) {
  if(options.runner&&options.runner!=="auto")return options.runner;
  if(options.vitestConfig||options.vitestPackage||options.vitestProject)return "vitest";
  if(configs(root,["vitest","vite"]))return resolvePackage(root,"vitest")?"vitest":resolvePackage(root,"vite-plus")?"vite-plus":"vitest";
  let pkg={};try{pkg=JSON.parse(readFileSync(join(root,"package.json"),"utf8"));}catch{}
  if(configs(root,["jest"])||Object.hasOwn(pkg,"jest"))return "jest";
  if(configs(root,["playwright"]))return "playwright";
  for(const [runner,name]of [["vitest","vitest"],["vite-plus","vite-plus"],["jest","jest"],["playwright","@playwright/test"]])
    if(resolvePackage(root,name))return runner;
  throw new Error("Cannot find a supported test runner; select --runner and --runner-package");
}
