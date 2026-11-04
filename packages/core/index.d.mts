export interface MutationPolicy {
  only?: string[];
  mutantsPerLine?: number | null;
  timeBudget?: number | null;
  failOnSurvived?: boolean;
  thresholdHigh?: number | null;
  thresholdLow?: number | null;
  thresholdBreak?: number | null;
  critical?: string[];
  acceptedFile?: string | null;
}
export interface MutationOptions {
  root?: string;
  node?: string;
  include?: string[];
  exclude?: string[];
  excludeMutator?: string[];
  vitestConfig?: string | null;
  vitestProject?: string | null;
  vitestPackage?: string | null;
  failOnUndetected?: boolean;
  typecheck?: boolean;
  weakMutation?: boolean;
  typecheckProject?: string | null;
  typecheckPackage?: string | null;
  policy?: MutationPolicy;
}
export type MutationConfig = MutationOptions & { $schema?: string };
export type MutationStatus = "Killed" | "Survived" | "NoCoverage" | "Timeout" |
  "RuntimeError" | "CompileError" | "Ignored" | "Pending";
export interface MutationTest {
  id: string;
  taskId: string;
  file: string;
  name: string;
  status: "passed" | "failed" | "skipped" | "pending";
  started: boolean;
  duration: number;
  errors: string[];
  hits: Record<string, number>;
  infected?: string[];
}
export interface MutationAttempt {
  number: number;
  status: MutationStatus;
  reason: string;
  killedBy: string[];
  failedModules: string[];
}
export interface Mutant {
  key: string;
  file: string;
  line: number;
  byteOffset: number;
  end: number;
  mutator: string;
  original: string;
  replacement: string;
  placement: "expression" | "statement" | "body" | "block" | "emptyCondition";
  ignored: string | null;
  regex: [string, string] | null;
  guardOffset: number | null;
  weak?: { operator: string; replacementOperator: string; leftEnd: number; rightStart: number };
  status: MutationStatus;
  coveredBy: string[];
  killedBy: string[];
  failedModules: string[];
  attempts: MutationAttempt[];
  reason?: string;
  accepted?: string;
  critical?: boolean;
  infectedBy?: string[];
}
export interface MutationReport {
  kind: "errspec/mutation";
  schemaVersion: 1;
  engine: {
    tsrsRevision: string; rulesVersion: string;
    node: string | null; vitest: string | null;
    typecheck: string; coverageScope: string;
  };
  runComplete: boolean;
  ok: boolean;
  testsExecuted: boolean;
  baseline: MutationTest[];
  summary: {
    total: number; killed: number; survived: number; noCoverage: number;
    timeout: number; runtimeError: number; compileError?: number;
    ignored: number; pending: number; score: number | null;
  };
  diagnostics: { code: string; message: string }[];
  mutants: Mutant[];
  gate: { enabled: boolean; passed: boolean | null };
  typecheck?: {
    version: string; package: string; project: string;
    checked: number; compileErrors: number; equivalent: number;
  };
  policy?: {
    options: MutationPolicy;
    sampled: number; budget: number;
    acceptance: { applied: string[]; detected: string[]; stale: string[] };
    scoreLevel: "high" | "low" | "danger" | null;
    qualityFailures: string[];
  };
}
export interface MutationTransport {
  /** Override native binary discovery; useful for a locally built errspec. */
  binary?: string;
  cwd?: string;
  configFile?: string;
  signal?: AbortSignal;
}
/** Identity helper with configuration type checking; runtime validation is native. */
export declare function defineConfig(config: MutationConfig): MutationConfig;
/** Quality/configuration failures return their report; transport failures reject. */
export declare function runMutation(options?: MutationConfig, transport?: MutationTransport): Promise<{
  exitCode: number;
  report: MutationReport;
}>;
