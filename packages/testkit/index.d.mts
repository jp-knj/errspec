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
