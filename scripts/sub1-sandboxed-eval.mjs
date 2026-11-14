/**
 * Narrow evaluator process for agent-authored ESM. Node Permission Model
 * denies writes, arbitrary reads, network, child processes and native addons.
 * Do not import untrusted agent output into the privileged benchmark process.
 * https://nodejs.org/api/permissions.html (2026 Node 24)
 */
import {spawnSync} from "node:child_process";
import {join,resolve,sep} from "node:path";
import {realpathSync} from "node:fs";

export function sandboxedModuleOracle(relative,workspace){
 let file;
 try{
  const allowed=realpathSync(workspace);
  file=realpathSync(resolve(join(workspace,relative)));
  if(!file.startsWith(allowed+sep))return false;
 }catch{return false}
 const test=[
  "import {pathToFileURL} from 'node:url';",
  "const m=await import(pathToFileURL(process.argv[1]).href);",
  "const f=m.totalWithTax;",
  "if(typeof f!=='function'||m.currency!=='KRW') process.exit(3);",
  "const expected=[[[100,0.1],110],[[100.45,0.05],105],[[0,0],0],",
  " [[-1,0.1],null],[[100,-0.1],null],[[Infinity,0.2],null],",
  " [[NaN,0.1],null],[['100',0.1],null],[[100,null],null]];",
  "for(const [args,want] of expected){if(!Object.is(f(...args),want)) process.exit(4)}",
 ].join("\n");
 const result=spawnSync(process.execPath,[
  "--permission","--allow-fs-read="+file,
  "--input-type=module","--eval",test,file
 ],{
  cwd:workspace,encoding:"utf8",timeout:20000,maxBuffer:65536,
  env:{PATH:process.env.PATH||"",HOME:"/nonexistent",NODE_OPTIONS:""},
 });
 return result.status===0 && !result.error;
}
