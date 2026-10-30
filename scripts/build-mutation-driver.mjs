import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const directory=resolve(dirname(fileURLToPath(import.meta.url)),"../packages/mutation-driver");
const bun=process.env.BUN_BIN||"bun";
const version=spawnSync(bun,["--version"],{encoding:"utf8"});
if(version.status!==0||version.stdout.trim()!=="1.4.4")throw new Error("Build the embedded mutation driver with Bun 1.4.4");
const temporary=mkdtempSync(join(tmpdir(),"errspec-driver-build-"));
try {
  const bundle=join(temporary,"driver.mjs");
  const build=spawnSync(bun,["build","driver.mjs","--target=node","--format=esm","--outfile",bundle],{cwd:directory,encoding:"utf8"});
  if(build.status!==0)throw new Error(build.stderr||"Mutation driver build failed");
  const notices=["magic-string","@jridgewell/sourcemap-codec"].map(name=>{
    const path=join(directory,"node_modules",name);
    const pkg=JSON.parse(readFileSync(join(path,"package.json"),"utf8"));
    return `===== ${pkg.name} ${pkg.version} (${pkg.license}) =====\n${readFileSync(join(path,"LICENSE"),"utf8")}\n`;
  }).join("\n").trimEnd()+"\n";
  const expected=[[join(directory,"dist/driver.mjs"),readFileSync(bundle)],[join(directory,"THIRD_PARTY_NOTICES.txt"),Buffer.from(notices)]];
  if(process.argv.includes("--check")) {
    for(const [path,content] of expected)if(!readFileSync(path).equals(content))throw new Error(`Rebuild committed mutation artifact: ${path}`);
  } else for(const [path,content] of expected)writeFileSync(path,content);
} finally {rmSync(temporary,{recursive:true,force:true});}
