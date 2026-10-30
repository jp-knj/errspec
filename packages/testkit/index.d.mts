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
export function scenarioTestName(contract: string, id: string): string;
export function scenarioTest(
  test: RegisterTest,
  file: string | URL,
  definition: ScenarioDefinition,
  body: (scope: ScenarioScope) => void | Promise<void>,
): unknown;

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
  constructor(message: string, report: FailureAtomicityReport<unknown, unknown>);
}
export function checkFailureAtomicity<R, S>(options: FailureAtomicityOptions<R, S>): Promise<FailureAtomicityReport<R, S>>;
