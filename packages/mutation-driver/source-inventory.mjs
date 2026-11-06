// Shared input acquisition. Runner-specific enumeration and validation stay in the driver.
import {readFileSync} from "node:fs";
import {dirname,join} from "node:path";

export function registerSources(inventory,files,remember) {
  for(const file of files) {
    inventory.set(file,{source:readFileSync(file,"utf8"),candidates:[]});
    remember(file);
  }
}

export async function generatePluginSources(inventory,generate,relative) {
  for(const [file,entry] of inventory)entry.generated=await generate(entry.source,relative(file));
}

export function rememberProjectManifests(root,remember) {
  for(let directory=root;;directory=dirname(directory)) {
    for(const name of ["package.json","package-lock.json","bun.lock","bun.lockb","pnpm-lock.yaml","yarn.lock","tsconfig.json"])
      remember(join(directory,name));
    if(dirname(directory)===directory)break;
  }
}
