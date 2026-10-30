import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { runInNewContext } from "node:vm";
import { stripTypeScriptTypes } from "node:module";
import { instrument, byteIndices } from "../packages/mutation-driver/emitter.mjs";

const binary = process.env.ERRSPEC_BIN || new URL("../target/debug/errspec", import.meta.url).pathname;
function generate(source,file="example.ts") {
  const result=spawnSync(binary,["__mutation-source"],{input:JSON.stringify({source,file}),encoding:"utf8"});
  assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout).candidates;
}
function validate(source,file="example.ts") {
  const result=spawnSync(binary,["__mutation-source"],{input:JSON.stringify({source,file,validateOnly:true}),encoding:"utf8"});
  assert.equal(result.status,0,result.stderr);
}
test("UTF-8 offsets map exactly to UTF-16 including BOM, emoji, and CRLF",()=>{
  const text="\uFEFF日本🧪\r\nx";const map=byteIndices(text);
  assert.equal(map.get(Buffer.byteLength(text)),text.length);
  assert.equal(map.has(1),false);
  assert.equal(map.get(Buffer.byteLength("\uFEFF日本🧪")),5);
});
test("inactive switches preserve receivers, evaluation counts, short circuit, labels, and directives",()=>{
  const source=`"use strict";
let count=0;
const receiver={n:3, inc(){ count++; return this.n + 1; }};
const value=receiver?.inc();
const fallback=undefined?.missing.deep;
const a=false && receiver.inc();
label: for(let i=0;i<2;i++){ if(i===0)continue label; count++; }
function strict(){// keep the directive after a comment
"use strict"
return this;
}
globalThis.answer=[value,fallback,a,count,strict()];`;
  const candidates=generate(source,"example.js");
  const emitted=instrument(source,candidates,"example.js","test");validate(emitted.code,"example.js");
  const context={};context[Symbol.for("errspec.mutation.test")]=()=>false;
  runInNewContext(emitted.code,context);
  assert.deepEqual(Array.from(context.answer),[4,undefined,false,2,undefined]);
  const optional=candidates.find(c=>c.mutator==="OptionalChaining"&&c.original==="receiver?.inc()");
  assert(optional);context[Symbol.for("errspec.mutation.test")]=key=>key===optional.key;
  runInNewContext(emitted.code,{[Symbol.for("errspec.mutation.test")]:key=>key===optional.key});
  assert.equal(emitted.map.sourcesContent[0],source);
});
test("nested expressions and every supported rule produce syntactically valid switches",()=>{
  const source=`\uFEFF// 日本語 🧪\r\n
import {x} from "module";
type Label="type";
async function f(a:number):Promise<number>{ "use strict"; if(a>=2&&true){ console.log("ok"); } return a+1; }
const arr=[1,2]; const empty=Array(); const obj={a:1};
const arrow=(x:number):number=>-x;
let n=1; n+=2; n++; --n; !true; ~n;
const p=obj?.a; const q=obj?.["a"]; const fn=arrow?.(1);
const regex=/^\\d{2,4}$/i; const re=new RegExp("\\\\w+","g");
const method=arr.slice(1).filter(x=>x>0); const template=\`hello \${n}\`;
for(;;){break;} while(n>2){break;}
`;
  const candidates=generate(source);const emitted=instrument(source,candidates,"example.ts","test");validate(emitted.code);
  assert.equal(new Set(candidates.map(c=>c.mutator)).size,18);
  assert.match(emitted.code,/日本語 🧪\r\n/);
});
test("each active switch has the same result as applying that replacement directly",()=>{
  const source=`function compute(n){
    let x=n+1; x+=2; x++; const list=[x]; list.push(2);
    const object={n:x, get(){return this.n;}};
    const number=object?.get(); const mapped=list.slice(0).filter(v=>v>0);
    const arrow=v=>-v; const flag=!false;
    if(n>=2 && flag) { x=arrow(x); }
    return [x,number,mapped,flag,/^x+$/.test("xxx"),"hello"];
  }
  globalThis.answer=compute(3);`;
  const candidates=generate(source,"example.js");
  const emitted=instrument(source,candidates,"example.js","equivalence");
  const indices=byteIndices(source);
  function result(code,key) {
    const context={ [Symbol.for("errspec.mutation.equivalence")]: k=>k===key };
    try {runInNewContext(code,context,{timeout:100});return JSON.stringify(context.answer);}
    catch(error) {return `${error.name}: ${error.message}`;}
  }
  for(const candidate of candidates.filter(c=>!c.ignored)) {
    const replacement=source.slice(0,indices.get(candidate.byteOffset))+candidate.replacement+source.slice(indices.get(candidate.end));
    assert.equal(result(emitted.code,candidate.key),result(replacement,null),`${candidate.mutator}: ${candidate.original} -> ${candidate.replacement}`);
  }
});
test("JSX expressions are traversed while literal attributes and type syntax are excluded",()=>{
  const source='type Label="type";export const View=({n}:{n:number})=><div title="literal" data-n={n+1}>{true?"yes":"no"}</div>;';
  const candidates=generate(source,"example.tsx");
  assert(candidates.some(c=>c.mutator==="ArithmeticOperator"&&c.original==="n+1"));
  assert(!candidates.some(c=>c.original==='"literal"'||c.original==='"type"'));
  validate(instrument(source,candidates,"example.tsx","jsx").code,"example.tsx");
});
test("class extends expressions are runtime syntax; implements and generic arguments remain types",()=>{
  const source='declare function base<T>(name:string):new()=>{};class Derived extends base<"type">("runtime") implements Contract<"interface"> {}';
  const candidates=generate(source);
  assert(candidates.some(c=>c.mutator==="StringLiteral"&&c.original==='"runtime"'));
  assert(!candidates.some(c=>c.original==='"type"'||c.original==='"interface"'));
  validate(instrument(source,candidates,"example.ts","heritage").code);
});
test("typed function-return switches preserve async and Promise return behavior",async()=>{
  const source=`async function asyncValue(): Promise<boolean> {return true;}
function promiseValue(): Promise<number> {return Promise.resolve(2);}
function arrayValue(): readonly string[] {return ["x"];}
function bigintValue(): bigint {return 2n;}
globalThis.answer=Promise.all([asyncValue(),promiseValue(),arrayValue(),bigintValue()]);`;
  const candidates=generate(source);
  const emitted=stripTypeScriptTypes(instrument(source,candidates,"example.ts","returns").code);
  const indices=byteIndices(source);
  async function result(code,key) {
    const context={ [Symbol.for("errspec.mutation.returns")]: k=>k===key };
    runInNewContext(code,context,{timeout:100});
    return JSON.stringify(await context.answer,(_key,value)=>typeof value==="bigint"?`${value}n`:value);
  }
  for(const candidate of candidates.filter(c=>c.mutator==="FnValue"&&!c.ignored)) {
    const replacement=source.slice(0,indices.get(candidate.byteOffset))+candidate.replacement+source.slice(indices.get(candidate.end));
    assert.equal(await result(emitted,candidate.key),await result(stripTypeScriptTypes(replacement),null),candidate.replacement);
  }
});
