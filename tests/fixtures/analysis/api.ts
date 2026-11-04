import { renamed } from './barrel'; const alias = renamed;
export function api(): void { alias(); }
export function a(n:number): void { if(n>0) b(n-1); else api(); }
export function b(n:number): void { a(n); }
export function caught(): void { try { api(); } catch {} }
export function rethrow(): void { try { api(); } catch(e) { throw e; } }
export function shadow(renamed: () => void): void { renamed(); }
