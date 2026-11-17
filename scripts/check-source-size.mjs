// Prevent another multi-thousand-line UI or Rust module from accumulating.
// Run via: node scripts/check-source-size.mjs (no dependencies or toolchain).
import {readdir,readFile} from 'node:fs/promises';
import {join,relative,resolve,sep} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const MAX_LINES=900;
const directories=['src','web/src','sdk/typescript/src'];
const candidates=[];
async function collect(dir){
 for(const entry of await readdir(dir,{withFileTypes:true})){
  if(entry.name==='node_modules'||entry.name==='target'||entry.name==='dist')continue;
  const path=join(dir,entry.name);
  if(entry.isDirectory())await collect(path);
  else if(entry.isFile() && /\.(rs|ts|tsx)$/.test(entry.name)){
   const rel=relative(root,path).split(sep).join('/');
   // Localized strings are data dictionaries, not executable control flow.
   if(rel==='web/src/i18n/ko.ts'||rel==='web/src/i18n/en.ts')continue;
   candidates.push({path,rel});
  }
 }
}
for(const dir of directories)await collect(join(root,dir));
let failed=0;
const measured=[];
for(const {path,rel} of candidates){
 const text=await readFile(path,'utf8');
 const lines=text.endsWith('\n')?text.split('\n').length-1:text.split('\n').length;
 measured.push({path:rel,lines});
 if(lines>MAX_LINES){
  console.error(`FAIL ${rel}: ${lines} lines exceeds ${MAX_LINES}; split by responsibility`);
  failed++;
 }
}
measured.sort((a,b)=>b.lines-a.lines);
console.log(`Source complexity gate: ${measured.length} files scanned; hard limit ${MAX_LINES} lines/file`);
for(const item of measured.slice(0,8))console.log(`  ${item.lines.toString().padStart(4)}  ${item.path}`);
if(failed)process.exitCode=1;
