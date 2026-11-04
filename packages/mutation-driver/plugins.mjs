// Acquire module callbacks and portable edits. Identity and placement belong to Rust.
import { readFileSync, existsSync, realpathSync } from "node:fs";
import { dirname, resolve, join, isAbsolute } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { createRequire, registerHooks } from "node:module";

const builtin = new Set(["ArithmeticOperator","ArrayDeclaration","ArrowFunction","AssignmentOperator","BlockStatement","BooleanLiteral","CallExpression","ConditionalExpression","EqualityOperator","FnValue","LogicalOperator","MethodExpression","ObjectLiteral","OptionalChaining","Regex","StringLiteral","UnaryOperator","UpdateOperator"]);
const fail = message => Object.assign(new Error(message), {code:"EC_MUTATION_CONFIG"});
const validName = name => typeof name === "string" && /^[A-Za-z0-9_.-]+$/.test(name);

export async function loadPlugins(specs, root, remember, excluded = []) {
  const mutators=[], ignorers=[], aridCallees=[], names=new Set(), ignoreNames=new Set();
  let cacheable=true;
  const tracked=new Set();
  function acquire(file, module=false, content) {
    file=realpathSync(file);remember(file,content);if(module)tracked.add(file);
    // Resolution is affected by package manifests as well as loaded source.
    for(let dir=dirname(file);;dir=dirname(dir)) {
      if(existsSync(join(dir,"package.json")))remember(join(dir,"package.json"));
      if(dirname(dir)===dir)break;
    }
  }
  function add(value, settings, origin, inheritedCache=false, inputRoot=root) {
    if(Array.isArray(value)){for(const part of value)add(part,settings,origin,inheritedCache,inputRoot);return;}
    if(!value || typeof value!=="object" || !validName(value.name))throw fail(`Plugin ${origin} must export a named mutator, ignorer or plugin`);
    const deterministic=inheritedCache||value.cache==="deterministic";
    cacheable&&=deterministic;
    if(value.inputs!==undefined && (!Array.isArray(value.inputs)||value.inputs.some(p=>typeof p!=="string"||!p)))throw fail(`Plugin ${value.name} inputs must be file paths`);
    for(const path of value.inputs||[])acquire(resolve(inputRoot,path));
    const context={settings,root,readInput(path){
      const file=resolve(inputRoot,path),content=readFileSync(file);
      acquire(file,false,content);
      return content.toString("utf8");
    }};
    if(typeof value.visit==="function") {
      if(builtin.has(value.name)||names.has(value.name))throw fail(`Custom mutator ${value.name} duplicates a built-in or custom mutator`);
      names.add(value.name);mutators.push({definition:value,context});return;
    }
    if(typeof value.shouldIgnore==="function") {
      if(ignoreNames.has(value.name))throw fail(`Duplicate plugin ignorer ${value.name}`);
      ignoreNames.add(value.name);ignorers.push({definition:value,context});return;
    }
    if(!value.mutators&&!value.ignorers&&!value.aridCallees)throw fail(`Plugin ${value.name} has no mutators, ignorers or aridCallees`);
    for(const key of ["mutators","ignorers","aridCallees"])if(value[key]!==undefined&&!Array.isArray(value[key]))throw fail(`Plugin ${value.name} ${key} must be an array`);
    for(const part of [...value.mutators||[],...value.ignorers||[]])add(part,settings,origin,deterministic,inputRoot);
    aridCallees.push(...value.aridCallees||[]);
  }
  // Synchronous hooks see the complete actual module closure, including package
  // exports and JSON imports. Unknown external reads require the purity opt-in.
  const observe=()=>registerHooks({
    resolve(specifier,context,next) {
      const result=next(specifier,context);
      if(result.url.startsWith("file:"))acquire(fileURLToPath(result.url),true);
      return result;
    },
    load(url,context,next) {
      if(url.startsWith("file:"))acquire(fileURLToPath(url),true);
      return next(url,context);
    },
  });
  const hooks=observe();
  try {
    const require=createRequire(join(root,"__errspec_plugin__.mjs"));
    for(const spec of specs||[]) {
      const module=typeof spec==="string"?spec:spec.module;
      const settings=typeof spec==="string"?null:spec.settings??null;
      const file=realpathSync(module.startsWith(".")||isAbsolute(module)?resolve(root,module):require.resolve(module));
      acquire(file,true);
      const loaded=await import(pathToFileURL(file).href);
      add(loaded.default??loaded,settings,module,false,dirname(file));
    }
  } catch(error) {throw fail(`Cannot load mutation plugin: ${error.message}`);}
  finally {hooks.deregister();}
  for(const name of excluded)if(!builtin.has(name)&&!names.has(name))throw fail(`Unknown mutator: ${name}`);
  return {names:[...names],aridCallees,cacheable,files:tracked,
    async edits(source,file,metadata) {
      const hooks=observe();
      try {
        const bytes=Buffer.from(source);
        const nodes=Object.freeze(metadata.nodes.map((node,id)=>Object.freeze({
          ...node,id,children:Object.freeze([...node.children]),
          text:bytes.subarray(node.start,node.end).toString("utf8"),
        })));
        const edits=[],ignores=[];
        const context=(base,node)=>({...base,file,source,
          parent:node.parent===null?null:nodes[node.parent],nodes,
          text(range){return bytes.subarray(range.start,range.end).toString("utf8");},
        });
        for(const node of nodes) {
          for(const {definition,context:base} of mutators) {
            if(excluded.includes(definition.name))continue;
            let result;
            try {result=await definition.visit(node,context(base,node));}
            catch(error){throw fail(`Plugin mutator ${definition.name} failed in ${file} at ${node.start}: ${error.message}`);}
            if(result==null)continue;
            if(!Array.isArray(result))throw fail(`Plugin mutator ${definition.name} must return an array of edits`);
            for(const edit of result) {
              const start=edit?.range?.start??node.start,end=edit?.range?.end??node.end;
              if(typeof edit?.replacement!=="string"||!Number.isSafeInteger(start)||!Number.isSafeInteger(end))throw fail(`Plugin mutator ${definition.name} returned an invalid edit`);
              edits.push({mutator:definition.name,anchorStart:node.start,anchorEnd:node.end,anchorKind:node.kind,start,end,replacement:edit.replacement});
            }
          }
          for(const {definition,context:base} of ignorers) {
            let reason;
            try {reason=await definition.shouldIgnore(node,context(base,node));}
            catch(error){throw fail(`Plugin ignorer ${definition.name} failed in ${file} at ${node.start}: ${error.message}`);}
            if(reason==null||reason===false)continue;
            if(typeof reason!=="string"||!reason.trim())throw fail(`Plugin ignorer ${definition.name} must return a nonempty reason`);
            ignores.push({start:node.start,end:node.end,reason:`${definition.name}: ${reason}`});
          }
        }
        return {pluginEdits:edits,pluginIgnores:ignores};
      } finally {hooks.deregister();}
    }};
}
