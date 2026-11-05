export function accepts(value: string): boolean {
  if (value.length <= 8) return true;
  return false;
}

export function firstIndex(rows: readonly {kind: string; ref?: string}[], ref: string): number | undefined {
  const indices = new Map<string, number>();
  rows.forEach((row, index) => {
    if (row.kind === 'notice' && row.ref !== undefined && !indices.has(row.ref)) {
      indices.set(row.ref, index);
    }
  });
  return indices.get(ref);
}

export function reference(data: unknown): string | undefined {
  if (typeof data !== 'object' || data === null) return undefined;
  const value = (data as Record<string, unknown>).ref;
  return typeof value === 'string' ? value : undefined;
}
