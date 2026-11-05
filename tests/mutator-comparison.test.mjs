import assert from 'node:assert/strict';
import test from 'node:test';
import {compareReports} from '../scripts/compare-mutation-reports.mjs';

const source='const label="🧪日本"; export const allowed = true;';
const start=source.indexOf('true'),end=start+4;
const candidate={file:'src/value.ts',mutator:'BooleanLiteral',original:'true',replacement:'false'};
const native={kind:'errspec/mutation',runComplete:true,mutants:[{...candidate,byteOffset:Buffer.byteLength(source.slice(0,start)),end:Buffer.byteLength(source.slice(0,end)),status:'Killed'}]};
const reference={entries:[{mutant:{...candidate,file:'/project/src/value.ts',range:{start,end}},status:'Killed'}]};
const compare=(a=native,b=reference)=>compareReports({native:a,reference:b,root:'/project',readSource:file=>{assert.equal(file,'src/value.ts');return source;}});
test('shared edits match across UTF-16 and UTF-8 ranges, independent of tool IDs',()=>{
  const result=compare();assert.equal(result.common,1);assert.equal(result.sameVerdict,1);assert.deepEqual(result.differences,[]);
  assert.equal(result.nativeOnly,0);assert.equal(result.referenceOnly,0);assert.deepEqual(result.commonByMutator,{BooleanLiteral:1});
});
test('different replacement spelling stays unmatched and missing verdicts never become detections',()=>{
  const changed=structuredClone(reference);changed.entries[0].mutant.replacement='(false)';
  const result=compare(native,changed);assert.equal(result.common,0);assert.equal(result.nativeOnly,1);assert.equal(result.referenceOnly,1);
  const pending=structuredClone(native);pending.mutants[0].status='Pending';pending.runComplete=false;
  const delta=compare({report:pending,elapsedMs:5},{report:reference,elapsedMs:2});
  assert.equal(delta.sameVerdict,0);assert.equal(delta.differences[0].nativeStatus,'Pending');assert.equal(delta.differences[0].referenceStatus,'Killed');
  assert.equal(delta.native.runComplete,false);assert.equal(delta.native.elapsedMs,5);
});
test('stale source, duplicate edits, split Unicode ranges and outside paths fail explicitly',()=>{
  const invalid=structuredClone(native);invalid.mutants[0].original='TRUE';assert.throws(()=>compare(invalid),/Source mismatch/);
  const duplicate=structuredClone(reference);duplicate.entries.push(duplicate.entries[0]);assert.throws(()=>compare(native,duplicate),/Duplicate exact edit/);
  const split=structuredClone(reference);split.entries[0].mutant.range.start=source.indexOf('🧪')+1;assert.throws(()=>compare(native,split),/surrogate pair/);
  const bytes=structuredClone(native);bytes.mutants[0].byteOffset=Buffer.byteLength(source.slice(0,source.indexOf('🧪')))+1;assert.throws(()=>compare(bytes),/splits a code point/);
  const outside=structuredClone(reference);outside.entries[0].mutant.file='/outside/src/value.ts';assert.throws(()=>compare(native,outside),/outside comparison root/);
});
