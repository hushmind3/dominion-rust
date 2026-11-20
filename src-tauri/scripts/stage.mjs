import {execFileSync} from 'node:child_process';
import {copyFileSync,existsSync,mkdirSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {resolve,dirname} from 'node:path';
// Stages PRE-BUILT assets only. Never compiles or downloads.
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const target=process.argv[2]||execFileSync('rustc',['-vV'],{encoding:'utf8'})
 .split('\n').find(x=>x.startsWith('host: '))?.slice(6).trim();
if(!target||!/^[a-z0-9_][a-z0-9_.-]{5,100}$/.test(target))throw Error('Invalid target triple');
const name=process.platform==='win32'?'dominion.exe':'dominion';
const candidate=resolve(root,'target',target,'release',name);
const binary=existsSync(candidate)?candidate:resolve(root,'target','release',name);
if(!existsSync(binary)||!existsSync(resolve(root,'web/dist/index.html')))
 throw Error('Already-built native binary or React assets missing');
const out=resolve(root,'src-tauri/binaries','dominion-'+target+(process.platform==='win32'?'.exe':''));
mkdirSync(dirname(out),{recursive:true});
copyFileSync(binary,out);
console.log('Staged: '+out);
