import {mkdtempSync,mkdirSync,realpathSync,rmSync,symlinkSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";

/** Own a temporary project for the lifetime of one Node test. */
export function temporaryProject(t,prefix,{canonical=true}={}) {
  const directory=mkdtempSync(join(tmpdir(),prefix));
  const root=canonical?realpathSync(directory):directory;
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  return root;
}

/** The common Vitest skeleton; each test owns its source, configuration and assertions. */
export function prepareVitestProject(root,repo) {
  mkdirSync(join(root,"src"));
  mkdirSync(join(root,"node_modules"));
  symlinkSync(join(repo,"examples/checkout/node_modules/vitest"),join(root,"node_modules/vitest"),"dir");
}
