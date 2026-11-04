
function leaf(): void { throw new Error('leaf'); }
function safe(): void {}
const typedAlias: typeof leaf = safe;
let mutable = leaf;
export function typed() { typedAlias(); }
export function callback(f: typeof leaf) { f(); }
export function reassigned() { mutable(); }
export function member(obj: { f: typeof leaf }) { obj.f(); }
export function customPromise(P: PromiseConstructor) { return P.reject('x'); }
const NativePromise = Promise;
export function nativeAlias() { return NativePromise.reject('x'); }
const arrow = () => { throw new Error('arrow'); };
export function directArrow() { arrow(); }
export async function mutablePromise() { let p = nativeAlias(); try { await p; } catch {} }
