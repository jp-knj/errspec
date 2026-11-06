// Generated from index.ts by scripts/build-testkit.mjs. Do not edit.
// Canonical source: packages/testkit/index.ts, also a standalone vendored TypeScript helper.
// scripts/build-testkit.mjs generates the package's JavaScript and declarations.
import { deepStrictEqual } from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
function nonempty(value) { return typeof value === "string" && value.trim().length > 0; }
function isThenable(value) {
    return typeof value?.then === "function";
}
export function scenarioTestName(contract, id) {
    if (!nonempty(contract) || !nonempty(id))
        throw new Error("Contract and scenario ids must be nonempty");
    return `errspec ${JSON.stringify([contract, id])}`;
}
/** Register at the top level or inside describe. Each invocation owns its observations, including in concurrent tests. */
export function scenarioTest(test, file, definition, body) {
    const { contract, id, tag, withoutFaults = false, description } = definition;
    if (description !== undefined && (!nonempty(description) || /[\r\n]/.test(description))) {
        throw new Error("Description must be a nonempty single line");
    }
    // CLI matching uses the stable id prefix; the description is for people reading runner output.
    const name = description === undefined
        ? scenarioTestName(contract, id)
        : `${scenarioTestName(contract, id)} ${description.trim()}`;
    if (typeof tag !== "string")
        throw new Error("Error tag must be a string");
    if (typeof withoutFaults !== "boolean")
        throw new Error("withoutFaults must be a boolean");
    const filename = fileURLToPath(file);
    return test(name, async () => {
        const directory = process.env["ERRSPEC_OBSERVATIONS"];
        const runId = process.env["ERRSPEC_RUN_ID"];
        if (Boolean(directory) !== Boolean(runId))
            throw new Error("Incomplete errspec execution environment");
        const path = directory ? join(directory, `${randomUUID()}.json`) : undefined;
        const record = {
            schemaVersion: 1, kind: "errspec/observation", runId: runId ?? "standalone",
            contractId: contract, scenarioId: id, tag, file: filename, testName: name,
            state: "running", faults: [], assertions: [],
        };
        const save = () => {
            if (!path)
                return;
            writeFileSync(`${path}.tmp`, JSON.stringify(record), { flag: "wx" });
            renameSync(`${path}.tmp`, path);
        };
        save();
        let closed = false;
        let violated = false;
        const reject = (message) => {
            violated = true;
            if (closed) {
                record.state = "incomplete";
                save();
            }
            throw new Error(message);
        };
        const active = () => { if (closed)
            reject("Scenario has already finished; await all scenario work"); };
        const scope = Object.freeze({
            fault(faultId, produceFailure) {
                active();
                if (withoutFaults)
                    reject("Scenario declared withoutFaults; remove the fault or the declaration");
                if (!nonempty(faultId) || record.faults.some(f => f.id === faultId) || typeof produceFailure !== "function") {
                    reject("Fault requires a unique nonempty id and a failure-producing function");
                }
                const observed = { id: faultId, calls: 0, completed: 0 };
                record.faults.push(observed);
                return async function (...args) {
                    active();
                    observed.calls++;
                    try {
                        return await produceFailure.apply(this, args);
                    }
                    finally {
                        observed.completed++;
                        if (closed) {
                            record.state = "incomplete";
                            save();
                        }
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
                    if (isThenable(actual) || isThenable(expected)) {
                        reject("Await values before comparing them");
                    }
                    deepStrictEqual(actual, expected);
                    observed.status = "passed";
                }
                catch (error) {
                    violated = true;
                    throw error;
                }
            },
        });
        try {
            await body(scope);
            if (violated)
                throw new Error("Scenario contained a failed check or invalid operation, even if caught");
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
        }
        catch (error) {
            if (record.state !== "incomplete")
                record.state = "failed";
            throw error;
        }
        finally {
            closed = true;
            save();
        }
    });
}
export class FailureAtomicityError extends Error {
    report;
    constructor(message, report) {
        super(message);
        this.name = "FailureAtomicityError";
        this.report = report;
    }
}
/** Explore single dependency failures with fresh fixtures and an explicit state oracle. */
export async function checkFailureAtomicity(options) {
    if (typeof options?.setup !== "function" || typeof options.isFailure !== "function"
        || !Array.isArray(options.faults) || options.faults.length === 0) {
        throw new TypeError("Provide setup, isFailure and at least one fault");
    }
    const ids = new Set();
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
    const report = { baseline: { calls: {} }, cases: [] };
    const message = (error) => error instanceof Error ? error.message : String(error);
    async function execute(selected, injection) {
        const calls = Object.create(null);
        let fixture;
        let pending = 0;
        let active = true;
        let injected = false;
        // The baseline is kept separately from fault cases in the public report.
        const trial = {
            fault: selected ?? { fault: "baseline", dependency: "", call: 0 }, status: "violated",
        };
        const scope = {
            wrap(dependency, implementation) {
                if (!nonempty(dependency) || typeof implementation !== "function") {
                    throw new TypeError("wrap requires a dependency name and an implementation");
                }
                return async function (...args) {
                    if (!active)
                        throw new Error("Dependency called after the operation finished");
                    calls[dependency] = (calls[dependency] ?? 0) + 1;
                    const inject = selected?.dependency === dependency && selected.call === calls[dependency];
                    pending++;
                    try {
                        if (inject) {
                            injected = true;
                            return await injection.inject.apply(this, args);
                        }
                        return await implementation.apply(this, args);
                    }
                    finally {
                        pending--;
                    }
                };
            },
        };
        try {
            fixture = await options.setup();
            trial.before = structuredClone(await fixture.snapshot());
            trial.result = await fixture.run(scope);
            active = false;
            if (pending !== 0)
                throw new Error("Operation finished with dependency calls still pending; await them");
            trial.after = structuredClone(await fixture.snapshot());
            if (selected && !injected)
                throw new Error("Selected fault was not injected");
            const failed = options.isFailure(trial.result);
            if (typeof failed !== "boolean")
                throw new TypeError("isFailure must return a boolean");
            if (!selected && failed)
                throw new Error("Baseline operation must succeed without an injected fault");
            if (selected && failed) {
                deepStrictEqual(trial.after, trial.before, "Failed operation changed the protected state");
                trial.status = "preserved";
            }
            else {
                trial.status = "recovered";
            }
        }
        catch (error) {
            trial.status = "violated";
            trial.error = error;
            trial.message = message(error);
            // Preserve the state left by unexpected throws as well, when possible.
            if (fixture && !Object.hasOwn(trial, "after") && pending === 0) {
                try {
                    trial.after = structuredClone(await fixture.snapshot());
                }
                catch { /* original diagnostic wins */ }
            }
        }
        finally {
            active = false;
            try {
                await fixture?.dispose?.();
            }
            catch (error) {
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
        if (only && only.fault !== fault.id)
            continue;
        const count = baseline.calls[fault.dependency] ?? 0;
        if (count === 0 || (only && only.call > count)) {
            report.cases.push({
                fault: { fault: fault.id, dependency: fault.dependency, call: only?.call ?? 1 },
                status: "violated", message: "Selected dependency call was not observed in the baseline",
            });
            continue;
        }
        for (let call = 1; call <= count; call++) {
            if (only && only.call !== call)
                continue;
            const selected = { fault: fault.id, dependency: fault.dependency, call };
            report.cases.push((await execute(selected, fault)).trial);
        }
    }
    const violations = report.cases.filter(trial => trial.status === "violated");
    if (violations.length) {
        throw new FailureAtomicityError(violations.map(trial => `${trial.fault.fault} at ${trial.fault.dependency} call ${trial.fault.call}: ${trial.message}`).join("\n"), report);
    }
    return report;
}
