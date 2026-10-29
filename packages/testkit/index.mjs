import { deepStrictEqual } from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

function nonempty(value) { return typeof value === "string" && value.trim().length > 0; }

export function scenarioTestName(contract, id) {
  if (!nonempty(contract) || !nonempty(id)) throw new Error("Contract and scenario ids must be nonempty");
  return `errspec ${JSON.stringify([contract, id])}`;
}

/** Register at the top level. Each invocation owns its observations, including in concurrent tests. */
export function scenarioTest(test, file, definition, body) {
  const { contract, id, tag, withoutFaults = false, description } = definition;
  if (description !== undefined && (!nonempty(description) || /[\r\n]/.test(description))) {
    throw new Error("Description must be a nonempty single line");
  }
  // CLI matching uses the stable id prefix; the description is for people reading runner output.
  const name = description === undefined
    ? scenarioTestName(contract, id)
    : `${scenarioTestName(contract, id)} ${description.trim()}`;
  if (typeof tag !== "string") throw new Error("Error tag must be a string");
  if (typeof withoutFaults !== "boolean") throw new Error("withoutFaults must be a boolean");
  const filename = fileURLToPath(file);
  return test(name, async () => {
    const directory = process.env.ERRSPEC_OBSERVATIONS;
    const runId = process.env.ERRSPEC_RUN_ID;
    if (Boolean(directory) !== Boolean(runId)) throw new Error("Incomplete errspec execution environment");
    const path = directory ? join(directory, `${randomUUID()}.json`) : undefined;
    const record = {
      schemaVersion: 1, kind: "errspec/observation", runId: runId ?? "standalone",
      contractId: contract, scenarioId: id, tag, file: filename, testName: name,
      state: "running", faults: [], assertions: [],
    };
    const save = () => {
      if (!path) return;
      writeFileSync(`${path}.tmp`, JSON.stringify(record), { flag: "wx" });
      renameSync(`${path}.tmp`, path);
    };
    save();
    let closed = false;
    let violated = false;
    const reject = (message) => {
      violated = true;
      if (closed) { record.state = "incomplete"; save(); }
      throw new Error(message);
    };
    const active = () => { if (closed) reject("Scenario has already finished; await all scenario work"); };
    const scope = Object.freeze({
      fault(faultId, produceFailure) {
        active();
        if (withoutFaults) reject("Scenario declared withoutFaults; remove the fault or the declaration");
        if (!nonempty(faultId) || record.faults.some(f => f.id === faultId) || typeof produceFailure !== "function") {
          reject("Fault requires a unique nonempty id and a failure-producing function");
        }
        const observed = { id: faultId, calls: 0, completed: 0 };
        record.faults.push(observed);
        return async function (...args) {
          active();
          observed.calls++;
          try { return await produceFailure.apply(this, args); }
          finally {
            observed.completed++;
            if (closed) { record.state = "incomplete"; save(); }
          }
        };
      },
      check(assertionId, actual, expected) {
        active();
        if (!nonempty(assertionId) || record.assertions.some(a => a.id === assertionId)) {
          reject("Assertion requires a unique nonempty id");
        }
        const observed = { id: assertionId, status: "failed" };
        record.assertions.push(observed);
        try {
          if (typeof actual?.then === "function" || typeof expected?.then === "function") {
            reject("Await values before comparing them");
          }
          deepStrictEqual(actual, expected);
          observed.status = "passed";
        } catch (error) { violated = true; throw error; }
      },
    });
    try {
      await body(scope);
      if (violated) throw new Error("Scenario contained a failed check or invalid operation, even if caught");
      if (record.faults.length === 0 && !withoutFaults) {
        record.state = "incomplete";
        throw new Error("Scenario has no faults; declare withoutFaults: true for an input-only scenario");
      }
      if (record.faults.some(f => f.calls === 0 || f.calls !== f.completed)) {
        record.state = "incomplete";
        throw new Error("Every configured fault must be invoked and settled before the scenario finishes");
      }
      if (record.assertions.length === 0) {
        record.state = "incomplete";
        throw new Error("Scenario has no completed assertions");
      }
      record.state = "passed";
    } catch (error) {
      if (record.state !== "incomplete") record.state = "failed";
      throw error;
    } finally { closed = true; save(); }
  });
}
