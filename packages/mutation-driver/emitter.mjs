import MagicString from "magic-string";

import { byteIndices } from "../typecheck/ranges.mjs";
export { byteIndices };

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
  function slice(group,start,end) {
    let original = source.slice(start,end);
    for(const child of [...group.children].reverse()) {
      if(child.end<=start||child.start>=end)continue;
      if(child.start<start||child.end>end)throw new Error(`Weak operands cross a mutation placement in ${file}`);
      original=original.slice(0,child.start-start)+render(child)+original.slice(child.end-start);
    }
    return original;
  }
  function render(group) {
    let original = slice(group,group.start,group.end);
    const weak = group.candidates.filter(c=>c.weak);
    if(weak.length) {
      const layout=weak[0].weak;
      const leftEnd=indices.get(layout.leftEnd),rightStart=indices.get(layout.rightStart);
      if(leftEnd===undefined||rightStart===undefined||leftEnd<group.start||rightStart>group.end||leftEnd>rightStart
        ||weak.some(c=>c.weak.leftEnd!==layout.leftEnd||c.weak.rightStart!==layout.rightStart||c.weak.operator!==layout.operator))throw new Error(`Invalid weak mutation range in ${file}`);
      const changes=JSON.stringify(weak.map(c=>[c.key,c.weak.replacementOperator]));
      const left=slice(group,group.start,leftEnd),right=slice(group,rightStart,group.end);
      original=`(${runtime}?.binary?${runtime}.binary(${JSON.stringify(layout.operator)},${changes},(${left}),(${right})):(${original}))`;
    }
    for (const c of [...group.candidates].reverse()) {
      if(c.weak)continue;
      const check = `${runtime}?.(${JSON.stringify(c.key)})`;
      if (c.placement === "emptyCondition") original = `(${check}?false:true)`;
      else if (c.placement === "expression") original = `(${check}?(${c.replacement}):(${original}))`;
      else if (c.placement === "body") {
        // Keep directives and the original lexical scope; no extra scope around declarations.
        const prefixLength=indices.get(c.guardOffset)-group.start;
        if(!Number.isInteger(prefixLength)||prefixLength<1)throw new Error("Invalid body guard position");
        const directive=original.slice(1,prefixLength);
        // A whole-body replacement may intentionally fall through. Finish the
        // function here instead of continuing into the original body.
        const branch = `${c.replacement.slice(1,-1).trim()}\nreturn;`;
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
