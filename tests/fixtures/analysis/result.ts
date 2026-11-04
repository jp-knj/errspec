
export type Failure={kind:'Missing'}|{kind:'Invalid'};
type Result<T,E>={ok:true,value:T}|{ok:false,error:E};
export function parse(x:string): Result<string,Failure> { return {ok:false,error:{kind:'Missing'}}; }
export async function wrapper() { return parse(''); }
export function overload(x:string): void;
export function overload(x:number): void;
export function overload(x:string|number): void { throw new TypeError('bad'); }
export function caller() { overload(''); }
