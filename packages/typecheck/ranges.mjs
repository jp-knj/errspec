// tsrs uses UTF-8 bytes; MagicString uses JavaScript UTF-16 code units.
export function byteIndices(source) {
  const map = new Map([[0, 0]]);
  let bytes = 0, units = 0;
  for (const c of source) {
    bytes += Buffer.byteLength(c); units += c.length; map.set(bytes, units);
  }
  return map;
}
