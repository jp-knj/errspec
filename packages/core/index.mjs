import { exchangeNative, jsonInput, resolveNativeBinary } from "./native-transport.mjs";

export function defineConfig(config) { return config; }
export function defineMutator(definition) { return definition; }
export function defineIgnorer(definition) { return definition; }
export function definePlugin(definition) { return definition; }

/** Run native planning/judgment directly; quality failure still returns its report. */
export async function runMutation(options = {}, transport = {}) {
  return invokeMutation(options,transport,"runMutation","errspec/mutation");
}
export async function selectTests(options = {}, transport = {}) {
  return invokeMutation(options,transport,"selectTests","errspec/test-selection");
}
async function invokeMutation(options,transport,operation,kind) {
  if (Number(process.versions.node.split(".")[0]) < 24) throw new Error("runMutation requires Node.js 24 or later");
  transport.signal?.throwIfAborted();
  const input = jsonInput({ protocol: 1, operation, options, ...(transport.configFile ? { configFile: transport.configFile } : {}) }, "Mutation options must contain JSON values only");
  const binary = await resolveNativeBinary(transport);
  const { code, response: result } = await exchangeNative({
    binary, command: "__mutation-api", cwd: transport.cwd, input, label: "Mutation API",
    outputLimit: 64 * 1024 * 1024, outputError: "Mutation API output exceeds 64 MiB",
    signal: transport.signal, interrupt: true,
  });
  if(result.protocol!==1 || result.exitCode!==code || result.report?.kind!==kind || result.report?.schemaVersion!==1)throw new Error("Invalid mutation API response");
  return {exitCode:result.exitCode,report:result.report};
}

/** Render a native report through the shared Rust formatter. No tests are executed. */
export async function formatReport(report, options = {}, transport = {}) {
  if (Number(process.versions.node.split('.')[0]) < 24) throw new Error('formatReport requires Node.js 24 or later');
  const reporter = options.reporter ?? 'text';
  const binary = await resolveNativeBinary(transport);
  const input = jsonInput({protocol:1,report,reporter,root:options.root??'.',sources:options.sources??{}}, 'Report options must contain JSON values only');
  if (Buffer.byteLength(input)>64*1024*1024) throw new Error('Report request exceeds 64 MiB');
  const { code, response: result } = await exchangeNative({
    binary, command: '__format-report', cwd: transport.cwd, input, label: 'Formatter',
    outputLimit: 128 * 1024 * 1024, outputError: 'Formatter output exceeds 128 MiB',
  });
  if(result.protocol!==1)throw new Error('Invalid formatter response');
  if(code!==0)throw new Error(result.error??'Report formatting failed');
  if(typeof result.output!=='string')throw new Error('Invalid formatter response');
  return result.output;
}
