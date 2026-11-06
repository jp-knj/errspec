export interface TypecheckOptions {
  root?: string;
  project?: string;
  typescriptPackage?: string;
}
export interface TypecheckMutant {
  key: string;
  file: string;
  /** UTF-8 byte offsets into the original file. */
  byteOffset: number;
  end: number;
  original: string;
  replacement: string;
  mutator?: string;
}
export interface TypecheckFacts {
  info: {version:string;package:string;project:string;checked:number;compileErrors:number;equivalent:number};
  outcomes: {key:string;diagnostic:string|null;equivalentReason:string|null}[];
}
/** Reports newly introduced diagnostics and narrow type-backed equivalence facts; never executes tests. */
export declare function checkMutants(options:TypecheckOptions,mutants:readonly TypecheckMutant[],progress?:(key:string)=>void):Promise<TypecheckFacts>;
export declare function checkMutantsInWorker(options:TypecheckOptions,mutants:readonly TypecheckMutant[],transport?:{signal?:AbortSignal}):Promise<TypecheckFacts>;
