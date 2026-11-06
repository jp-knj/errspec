import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";

function checked(result,expectedExit) {
  assert.ifError(result.error);
  assert.equal(result.status,expectedExit,result.stdout+result.stderr);
  return result;
}

/** Callers own arguments, cwd, environment, timeouts and the expected exit status. */
export function runCommand(binary,args,options,expectedExit=0) {
  return checked(spawnSync(binary,args,options),expectedExit);
}

export function parseJsonResult(result,expectedExit=0) {
  return JSON.parse(checked(result,expectedExit).stdout);
}

export function runJsonCommand(binary,args,options,expectedExit=0) {
  return parseJsonResult(spawnSync(binary,args,options),expectedExit);
}
