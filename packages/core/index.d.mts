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
  reporters?: MutationReporter[];
  reportDir?: string | null;
  plugins?: (string | { module: string; settings?: JsonValue })[];
  since?: string | null;
  diffUnit?: "node" | "scope" | null;
  shard?: { index: number; total: number } | null;
  concurrency?: number;
  arid?: boolean;
  aridCallees?: string[] | null;
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
  reuseSession?: boolean;
  cache?: boolean;
  cacheRelocation?: boolean;
  cacheScope?: boolean;
  experimentalCallgraph?: boolean;
  cacheDir?: string | null;
  fullDryRun?: boolean;
  typecheckProject?: string | null;
  typecheckPackage?: string | null;
  policy?: MutationPolicy;
}
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
/** Owned TypeScript syntax facts; start/end and edit ranges are UTF-8 byte offsets. */
export interface MutationNode {
  readonly id: number; readonly kind: string; readonly start: number; readonly end: number;
  readonly parent: number | null; readonly children: readonly number[];
  readonly name: string | null; readonly text: string;
}
export interface PluginContext {
  readonly file: string; readonly source: string; readonly root: string;
  readonly settings: JsonValue; readonly parent: MutationNode | null;
  readonly nodes: readonly MutationNode[];
  text(range: { start: number; end: number }): string;
  /** Read a file relative to the plugin module and acquire it for cache invalidation. */
  readInput(path: string): string;
}
export interface MutationEdit { replacement: string; range?: { start: number; end: number } }
export interface PluginInputs {
  name: string;
  /** Promise deterministic behavior from the acquired module closure, settings and inputs. */
  cache?: "deterministic";
  /** Additional paths relative to the module. Use readInput for computed paths. */
  inputs?: string[];
}
export interface MutatorDefinition extends PluginInputs {
  visit(node: MutationNode, context: PluginContext): MutationEdit[] | void | Promise<MutationEdit[] | void>;
}
export interface IgnorerDefinition extends PluginInputs {
  shouldIgnore(node: MutationNode, context: PluginContext): string | false | void | Promise<string | false | void>;
}
export interface MutationPlugin extends PluginInputs {
  mutators?: MutatorDefinition[];
  ignorers?: IgnorerDefinition[];
  aridCallees?: string[];
}
export function defineMutator<T extends MutatorDefinition>(definition: T): T;
export function defineIgnorer<T extends IgnorerDefinition>(definition: T): T;
export function definePlugin<T extends MutationPlugin>(definition: T): T;
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
  reused?: boolean;
  infectedBy?: string[];
}
export interface MutationReport {
  kind: "errspec/mutation";
  schemaVersion: 1;
  engine: {
    tsrsRevision: string; rulesVersion: string;
    node: string | null; vitest: string | null;
    typecheck: string; coverageScope: string;
    sessionReuse?: "available" | "fresh_global_setup" | "fresh_lifecycle_unknown";
    parallel?: { requested: number; workers: number };
  };
  runComplete: boolean;
  ok: boolean;
  testsExecuted: boolean;
  baseline: MutationTest[];
  childProcesses?: {test: string | null; file: string; executable: string; entries: string[]; method: "spawn" | "spawnSync" | "execSync" | "execFileSync"}[];
  summary: {
    total: number; killed: number; survived: number; noCoverage: number;
    timeout: number; runtimeError: number; compileError?: number;
    ignored: number; pending: number; score: number | null;
  };
  diagnostics: { code: string; message: string }[];
  mutants: Mutant[];
  workers?: { active: number; recovered: number; unverified: number; endpointUnavailable?: number; warnings?: string[] };
  shard?: { index: number; total: number; algorithm: "sha256_prefix_v1"; eligible: number; selected: number; omitted: number };
  cache?: {
    relocation?: boolean;
    directory: string; reused: number; executed: number; saved: number;
    invalidated: boolean; fingerprint?: string; disabledReason?: string; warnings?: string[];
    coverageReused?: boolean;
  };
  selection?: {
    since: string; base: string; unit: "node" | "scope";
    total: number; selected: number; omitted: number;
    changedFiles: string[]; fullRunReason: string | null;
    warnings: {code: string; file: string; message: string}[];
  };
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

export type MutationReporter = 'text' | 'json' | 'html' | 'agent' | 'simplify' | 'annotations';
export interface ReportFormatOptions {
  /** json is the mutation-testing-elements schema v2, distinct from the native report. */
  reporter?: MutationReporter;
  /** Resolve relative report paths here; defaults to the formatter working directory. */
  root?: string;
  /** Original UTF-8 source snapshots, keyed by report file. Required after source edits/removal. */
  sources?: Record<string,string>;
}
/** Same native formatter as the CLI; returns one string and does not write files or run tests. */
export declare function formatReport(report: MutationReport, options?: ReportFormatOptions,
  transport?: Pick<MutationTransport,'binary'|'cwd'>): Promise<string>;
