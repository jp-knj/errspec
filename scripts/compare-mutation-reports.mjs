// Compare exact edits across errspec and the fixed mutator reference. No semantic rewriting.
import {readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {isAbsolute, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';

const statuses=new Set(['Killed','Survived','NoCoverage','CompileError','Ignored','Pending','RuntimeError','Timeout']);
const increment=(counts,key)=>{counts[key]=(counts[key]??0)+1;};
function fileName(file,root) {
  if(typeof file!=='string')throw new Error('Missing mutant file');
  const name=(isAbsolute(file)?relative(root,file):file).split(sep).join('/');
  if(!name||name.includes('\\')||name.split('/').some(p=>!p||p==='.'||p==='..')||isAbsolute(name))throw new Error(`File outside comparison root: ${file}`);
  return name;
}
function utf8Offset(source,index) {
  if(!Number.isSafeInteger(index)||index<0||index>source.length)throw new Error('Invalid UTF-16 source range');
  const before=source.charCodeAt(index-1),after=source.charCodeAt(index);
  if(before>=0xd800&&before<=0xdbff&&after>=0xdc00&&after<=0xdfff)throw new Error('UTF-16 range splits a surrogate pair');
  return Buffer.byteLength(source.slice(0,index));
}
function entries(report,reference,root,readSource) {
  const values=reference?report.entries:report.mutants;
  if(!Array.isArray(values))throw new Error('Missing mutation inventory');
  const result=new Map();
  for(const value of values) {
    const mutant=reference?value.mutant:value;
    if(!mutant||!statuses.has(value.status)||typeof mutant.mutator!=='string'||typeof mutant.original!=='string'||typeof mutant.replacement!=='string')throw new Error('Invalid mutation entry');
    const file=fileName(mutant.file,root),source=readSource(file);
    if(typeof source!=='string')throw new Error(`Missing source: ${file}`);
    const start=reference?utf8Offset(source,mutant.range?.start):mutant.byteOffset;
    const end=reference?utf8Offset(source,mutant.range?.end):mutant.end;
    const bytes=Buffer.from(source);
    if(!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start<0||start>end||end>bytes.length)throw new Error(`Invalid UTF-8 source range: ${file}`);
    let original;
    try{original=new TextDecoder('utf-8',{fatal:true}).decode(bytes.subarray(start,end));}catch{throw new Error(`UTF-8 range splits a code point: ${file}`);}
    if(original!==mutant.original)throw new Error(`Source mismatch: ${file}:${start}`);
    const identity=JSON.stringify([file,start,end,mutant.mutator,mutant.original,mutant.replacement]);
    if(result.has(identity))throw new Error(`Duplicate exact edit: ${file}:${start} ${mutant.mutator}`);
    result.set(identity,{file,start,end,mutator:mutant.mutator,status:value.status});
  }
  return result;
}

export function compareReports({native,reference,root,readSource}) {
  const nativeRecord=native,referenceRecord=reference;
  native=native.report??native;reference=reference.report??reference;
  const left=entries(native,false,root,readSource),right=entries(reference,true,root,readSource);
  const nativeStatuses={},referenceStatuses={},commonByMutator={},nativeOnlyByMutator={},referenceOnlyByMutator={},differences=[];
  let common=0;
  for(const [identity,entry] of left) {
    increment(nativeStatuses,entry.status);
    const other=right.get(identity);
    if(!other){increment(nativeOnlyByMutator,entry.mutator);continue;}
    common++;increment(commonByMutator,entry.mutator);
    if(entry.status!==other.status)differences.push({...entry,nativeStatus:entry.status,referenceStatus:other.status});
  }
  for(const [identity,entry] of right) {
    increment(referenceStatuses,entry.status);
    if(!left.has(identity))increment(referenceOnlyByMutator,entry.mutator);
  }
  return {kind:'errspec/exact-mutator-comparison',schemaVersion:1,
    method:'Exact file, UTF-8 range, family, original and raw replacement; no semantic normalization',
    native:{candidates:left.size,statuses:nativeStatuses,elapsedMs:nativeRecord.elapsedMs??null,runComplete:native.runComplete??null},
    reference:{candidates:right.size,statuses:referenceStatuses,elapsedMs:referenceRecord.elapsedMs??null},
    common,sameVerdict:common-differences.length,differences,commonByMutator,
    nativeOnly:left.size-common,referenceOnly:right.size-common,nativeOnlyByMutator,referenceOnlyByMutator};
}

function main() {
  const {values}=parseArgs({options:{root:{type:'string'},native:{type:'string'},reference:{type:'string'},output:{type:'string'}}});
  if(!values.root||!values.native||!values.reference)throw new Error('Usage: node scripts/compare-mutation-reports.mjs --root PROJECT --native REPORT.json --reference REPORT.json [--output SUMMARY.json]');
  const root=realpathSync(values.root),sources=new Map();
  const readSource=file=>{
    if(!sources.has(file)) {
      const path=realpathSync(resolve(root,file)),name=relative(root,path);
      if(name==='..'||name.startsWith(`..${sep}`)||isAbsolute(name))throw new Error(`Source resolves outside comparison root: ${file}`);
      sources.set(file,readFileSync(path,'utf8'));
    }
    return sources.get(file);
  };
  const report=compareReports({native:JSON.parse(readFileSync(values.native,'utf8')),reference:JSON.parse(readFileSync(values.reference,'utf8')),root,readSource});
  const output=JSON.stringify(report,null,2)+'\n';
  if(values.output)writeFileSync(values.output,output);else process.stdout.write(output);
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  try{main();}catch(error){console.error(error.message);process.exitCode=2;}
}
