// TypeScript copy of index.mjs for projects that vendor the testkit next to typed tests.
// Behavior must match index.mjs; tests/testkit.test.mjs runs against both files.
import { deepStrictEqual } from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export interface ScenarioDefinition {
  readonly contract: string;
  readonly id: string;
  readonly tag: string;
  /**
   * The outcome depends only on input. Pair with `observe.faults: []` in the configuration.
   * The scenario then fails if it calls `fault`.
   */
  readonly withoutFaults?: boolean;
  /** Shown after the stable name in runner output. Changing it does not change the scenario. */
  readonly description?: string;
}

export interface ScenarioScope {
  /** Invocations of this wrapped dependency are observed, not its setup. */
  fault<Args extends unknown[], R>(id: string, produceFailure: (...args: Args) => R):
    (...args: Args) => Promise<Awaited<R>>;
  /** Real deep equality assertion. Await asynchronous values before passing them. */
  check<T>(id: string, actual: T, expected: NoInfer<T>): void;
}

/** Works with Bun test, Vitest test, and their skip/concurrent variants. */
export type RegisterTest = (name: string, body: () => Promise<void>) => unknown;

type FaultRecord = { id: string; calls: number; completed: number };
type AssertionRecord = { id: string; status: "passed" | "failed" };
type ObservationRecord = {
  schemaVersion: 1;
  kind: "errspec/observation";
  runId: string;
  contractId: string;
  scenarioId: string;
  tag: string;
  file: string;
  testName: string;
  state: "running" | "passed" | "failed" | "incomplete";
  faults: FaultRecord[];
  assertions: AssertionRecord[];
};

function nonempty(value: unknown): value is string { return typeof value === "string" && value.trim().length > 0; }

function isThenable(value: unknown): boolean {
  const candidate = typeof value === "object" && value !== null || typeof value === "function";
  return candidate && "then" in value && typeof value.then === "function";
}

export function scenarioTestName(contract: string, id: string): string {
  if (!nonempty(contract) || !nonempty(id)) throw new Error("Contract and scenario ids must be nonempty");
  return `errspec ${JSON.stringify([contract, id])}`;
}

/** Register at the top level. Each invocation owns its observations, including in concurrent tests. */
export function scenarioTest(
  test: RegisterTest,
  file: string | URL,
  definition: ScenarioDefinition,
  body: (scope: ScenarioScope) => void | Promise<void>,
): unknown {
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
    const directory = process.env["ERRSPEC_OBSERVATIONS"];
    const runId = process.env["ERRSPEC_RUN_ID"];
    if (Boolean(directory) !== Boolean(runId)) throw new Error("Incomplete errspec execution environment");
    const path = directory ? join(directory, `${randomUUID()}.json`) : undefined;
    const record: ObservationRecord = {
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
    const reject = (message: string): never => {
      violated = true;
      if (closed) { record.state = "incomplete"; save(); }
      throw new Error(message);
    };
    const active = () => { if (closed) reject("Scenario has already finished; await all scenario work"); };
    const scope: ScenarioScope = Object.freeze({
      fault<Args extends unknown[], R>(faultId: string, produceFailure: (...args: Args) => R) {
        active();
        if (withoutFaults) reject("Scenario declared withoutFaults; remove the fault or the declaration");
        if (!nonempty(faultId) || record.faults.some(f => f.id === faultId) || typeof produceFailure !== "function") {
          reject("Fault requires a unique nonempty id and a failure-producing function");
        }
        const observed: FaultRecord = { id: faultId, calls: 0, completed: 0 };
        record.faults.push(observed);
        return async function (this: unknown, ...args: Args): Promise<Awaited<R>> {
          active();
          observed.calls++;
          try { return await produceFailure.apply(this, args); }
          finally {
            observed.completed++;
            if (closed) { record.state = "incomplete"; save(); }
          }
        };
      },
      check<T>(assertionId: string, actual: T, expected: NoInfer<T>) {
        active();
        if (!nonempty(assertionId) || record.assertions.some(a => a.id === assertionId)) {
          reject("Assertion requires a unique nonempty id");
        }
        const observed: AssertionRecord = { id: assertionId, status: "failed" };
        record.assertions.push(observed);
        try {
          if (isThenable(actual) || isThenable(expected)) {
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
