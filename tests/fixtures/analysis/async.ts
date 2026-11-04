
export async function source(): Promise<void> { throw new Error('offline'); }
export async function outward(): Promise<void> { await source(); }
export async function caught(): Promise<void> { try { await source(); } catch {} }
export function returned() { try { return source(); } catch { return Promise.resolve(); } }
export async function aliasCaught() { const p = source(); try { await p; } catch {} }
export async function aliasReturned() { const p = source(); const q = p; return q; }
export function reject() { return Promise.reject(new Error('reject')); }
export function dangling() { source(); }
export function recursive(): Promise<void> { if (Math.random() > 0) throw new Error('cycle'); return bounce(); }
export async function bounce(): Promise<void> { await recursive(); }
