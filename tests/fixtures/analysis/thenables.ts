export type Failure = { _tag: 'Offline' };
type Result = { ok: false; error: Failure };
export function promiseLike(): PromiseLike<Result> {
  return Promise.resolve({ ok: false, error: { _tag: 'Offline' } });
}
export function custom() {
  return { then(fulfilled: (value: Result) => void) {
    fulfilled({ ok: false, error: { _tag: 'Offline' } });
  } };
}
export function union(): Promise<Result> | undefined {
  return Math.random() > 0 ? Promise.resolve({ ok: false, error: { _tag: 'Offline' } }) : undefined;
}
export function unusedThenable() { custom(); }
export function* generator() { throw new Error('iterate'); }
export function unusedGenerator() { generator(); }
export class Service {
  constructor() { if (Math.random() > 0) throw new Error('construct'); }
  method() { throw new Error('method'); }
  get value(): number { throw new Error('get'); }
}
export function dispatch() { const service = new Service(); service.method(); }
