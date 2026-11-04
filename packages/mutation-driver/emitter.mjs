import MagicString from "magic-string";

// tsrs uses UTF-8 bytes; MagicString uses JavaScript UTF-16 code units.
export function byteIndices(source) {
  const map = new Map([[0, 0]]);
  let bytes = 0, units = 0;
  for (const c of source) {
    bytes += Buffer.byteLength(c); units += c.length; map.set(bytes, units);
  }
  return map;
}

export function instrument(source, candidates, file, runId) {
  const indices = byteIndices(source);
  const root = { start: 0, end: source.length, children: [], candidates: [] };
  const groups = new Map();
  for (const candidate of candidates.filter(c => !c.ignored)) {
    const start = indices.get(candidate.byteOffset), end = indices.get(candidate.end);
    if (start === undefined || end === undefined || start > end)
      throw new Error(`Invalid UTF-8 range in ${file}`);
    const groupKey = `${start}:${end}`;
    let group = groups.get(groupKey);
    if (!group) {
      group = { start, end, children: [], candidates: [] }; groups.set(groupKey, group);
    }
    group.candidates.push(candidate);
  }
  const stack = [root];
  for (const group of [...groups.values()].sort((a,b) => a.start-b.start || b.end-a.end)) {
    while (stack.length > 1 && group.start >= stack.at(-1).end) stack.pop();
    const parent = stack.at(-1);
    if (group.end > parent.end) throw new Error(`Crossing mutation placements in ${file}`);
    parent.children.push(group); stack.push(group);
  }
  const runtime = `globalThis[Symbol.for(${JSON.stringify(`errspec.mutation.${runId}`)})]`;
  function render(group) {
    let original = source.slice(group.start,group.end);
    // Build inactive children first. Selecting the parent never evaluates them.
    for (const child of [...group.children].reverse()) {
      original = original.slice(0,child.start-group.start) + render(child)
        + original.slice(child.end-group.start);
    }
    for (const c of [...group.candidates].reverse()) {
      const check = `${runtime}?.(${JSON.stringify(c.key)})`;
      if (c.placement === "emptyCondition") original = `(${check}?false:true)`;
      else if (c.placement === "expression") original = `(${check}?(${c.replacement}):(${original}))`;
      else if (c.placement === "body") {
        // Keep directives and the original lexical scope; no extra scope around declarations.
        const prefixLength=indices.get(c.guardOffset)-group.start;
        if(!Number.isInteger(prefixLength)||prefixLength<1)throw new Error("Invalid body guard position");
        const directive=original.slice(1,prefixLength);
        const branch = c.replacement.slice(1,-1).trim() || "return;";
        original = `{${directive}\nif(${check}){${branch}}${original.slice(prefixLength)}`;
      } else {
        original = `{if(${check}){${c.replacement}}else{${original}}}`;
      }
    }
    return original;
  }
  const magic = new MagicString(source);
  for (const child of root.children) {
    if(child.start===child.end)magic.appendLeft(child.start,render(child));
    else magic.overwrite(child.start,child.end,render(child));
  }
  return { code: magic.toString(), map: magic.generateMap({source:file,includeContent:true,hires:true}) };
}
