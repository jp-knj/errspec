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

/** Register at the top level or inside describe. Each invocation owns its observations, including in concurrent tests. */
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

export interface FailureInjection {
  readonly id: string;
  readonly dependency: string;
  /** Supplies a valid failure for this dependency; may return or throw. */
  readonly inject: (...args: unknown[]) => unknown;
}

export interface FailureScope {
  wrap<Args extends unknown[], R>(dependency: string, implementation: (...args: Args) => R):
    (...args: Args) => Promise<Awaited<R>>;
}

export interface AtomicityFixture<R, S> {
  run(scope: FailureScope): R | Promise<R>;
  snapshot(): S | Promise<S>;
  dispose?(): void | Promise<void>;
}

export interface FailureAtomicityOptions<R, S> {
  readonly setup: () => AtomicityFixture<R, S> | Promise<AtomicityFixture<R, S>>;
  readonly faults: readonly FailureInjection[];
  readonly isFailure: (result: R) => boolean;
  /** Replays one case reported in FailureAtomicityError.report. */
  readonly only?: { readonly fault: string; readonly call: number };
}

export interface AtomicityFault {
  readonly fault: string;
  readonly dependency: string;
  readonly call: number;
}

export interface AtomicityCase<R, S> {
  readonly fault: AtomicityFault;
  status: "preserved" | "recovered" | "violated";
  before?: S;
  after?: S;
  result?: R;
  message?: string;
  error?: unknown;
}

export interface FailureAtomicityReport<R, S> {
  baseline: { calls: Record<string, number>; message?: string; error?: unknown };
  cases: AtomicityCase<R, S>[];
}

export class FailureAtomicityError extends Error {
  readonly report: FailureAtomicityReport<unknown, unknown>;
  constructor(message: string, report: FailureAtomicityReport<unknown, unknown>) {
    super(message);
    this.name = "FailureAtomicityError";
    this.report = report;
  }
}

/** Explore single dependency failures with fresh fixtures and an explicit state oracle. */
export async function checkFailureAtomicity<R, S>(
  options: FailureAtomicityOptions<R, S>,
): Promise<FailureAtomicityReport<R, S>> {
  if (typeof options?.setup !== "function" || typeof options.isFailure !== "function"
    || !Array.isArray(options.faults) || options.faults.length === 0) {
    throw new TypeError("Provide setup, isFailure and at least one fault");
  }
  const ids = new Set<string>();
  for (const fault of options.faults) {
    if (!fault || !nonempty(fault.id) || ids.has(fault.id) || !nonempty(fault.dependency)
      || typeof fault.inject !== "function") {
      throw new TypeError("Faults require unique nonempty ids, a dependency and an inject function");
    }
    ids.add(fault.id);
  }
  const only = options.only;
  if (only && (!ids.has(only.fault) || !Number.isSafeInteger(only.call) || only.call < 1)) {
    throw new TypeError("only must name a registered fault and a positive call number");
  }
  const report: FailureAtomicityReport<R, S> = { baseline: { calls: {} }, cases: [] };
  const message = (error: unknown) => error instanceof Error ? error.message : String(error);

  async function execute(selected?: AtomicityFault, injection?: FailureInjection) {
    const calls: Record<string, number> = Object.create(null);
    let fixture: AtomicityFixture<R, S> | undefined;
    let pending = 0;
    let active = true;
    let injected = false;
    // The baseline is kept separately from fault cases in the public report.
    const trial: AtomicityCase<R, S> = {
      fault: selected ?? { fault: "baseline", dependency: "", call: 0 }, status: "violated",
    };
    const scope: FailureScope = {
      wrap<Args extends unknown[], V>(dependency: string, implementation: (...args: Args) => V) {
        if (!nonempty(dependency) || typeof implementation !== "function") {
          throw new TypeError("wrap requires a dependency name and an implementation");
        }
        return async function (this: unknown, ...args: Args): Promise<Awaited<V>> {
          if (!active) throw new Error("Dependency called after the operation finished");
          calls[dependency] = (calls[dependency] ?? 0) + 1;
          const inject = selected?.dependency === dependency && selected.call === calls[dependency];
          pending++;
          try {
            if (inject) {
              injected = true;
              return await injection!.inject.apply(this, args) as Awaited<V>;
            }
            return await implementation.apply(this, args);
          } finally { pending--; }
        };
      },
    };
    try {
      fixture = await options.setup();
      trial.before = structuredClone(await fixture.snapshot());
      trial.result = await fixture.run(scope);
      active = false;
      if (pending !== 0) throw new Error("Operation finished with dependency calls still pending; await them");
      trial.after = structuredClone(await fixture.snapshot());
      if (selected && !injected) throw new Error("Selected fault was not injected");
      const failed = options.isFailure(trial.result);
      if (typeof failed !== "boolean") throw new TypeError("isFailure must return a boolean");
      if (!selected && failed) throw new Error("Baseline operation must succeed without an injected fault");
      if (selected && failed) {
        deepStrictEqual(trial.after, trial.before, "Failed operation changed the protected state");
        trial.status = "preserved";
      } else {
        trial.status = "recovered";
      }
    } catch (error) {
      trial.status = "violated";
      trial.error = error;
      trial.message = message(error);
      // Preserve the state left by unexpected throws as well, when possible.
      if (fixture && !Object.hasOwn(trial, "after") && pending === 0) {
        try { trial.after = structuredClone(await fixture.snapshot()); } catch { /* original diagnostic wins */ }
      }
    } finally {
      active = false;
      try { await fixture?.dispose?.(); } catch (error) {
        trial.status = "violated";
        trial.error = error;
        trial.message = `${trial.message ? `${trial.message}; ` : ""}Fixture cleanup failed: ${message(error)}`;
      }
    }
    return { calls, trial };
  }

  const baseline = await execute();
  report.baseline.calls = baseline.calls;
  if (baseline.trial.status === "violated") {
    report.baseline.message = baseline.trial.message ?? "Baseline failed";
    report.baseline.error = baseline.trial.error;
    throw new FailureAtomicityError(`Baseline failed: ${report.baseline.message}`, report);
  }
  for (const fault of options.faults) {
    if (only && only.fault !== fault.id) continue;
    const count = baseline.calls[fault.dependency] ?? 0;
    if (count === 0 || (only && only.call > count)) {
      report.cases.push({
        fault: { fault: fault.id, dependency: fault.dependency, call: only?.call ?? 1 },
        status: "violated", message: "Selected dependency call was not observed in the baseline",
      });
      continue;
    }
    for (let call = 1; call <= count; call++) {
      if (only && only.call !== call) continue;
      const selected = { fault: fault.id, dependency: fault.dependency, call };
      report.cases.push((await execute(selected, fault)).trial);
    }
  }
  const violations = report.cases.filter(trial => trial.status === "violated");
  if (violations.length) {
    throw new FailureAtomicityError(violations.map(trial =>
      `${trial.fault.fault} at ${trial.fault.dependency} call ${trial.fault.call}: ${trial.message}`,
    ).join("\n"), report);
  }
  return report;
}
