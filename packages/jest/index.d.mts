import type {MutationOptions,MutationTransport,MutationReport} from "@errspec/core";
export {defineConfig} from "@errspec/core";
export type {MutationReport,MutationTransport} from "@errspec/core";
export type RunnerOptions = Omit<MutationOptions,"runner"> & {runner?:"jest"};
export declare function runMutation(options?:RunnerOptions,transport?:MutationTransport):Promise<{exitCode:number;report:MutationReport}>;
