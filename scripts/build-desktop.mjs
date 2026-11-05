// Native desktop build, on macOS or Windows. No Node files are bundled.
import {spawnSync} from 'node:child_process';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const args=process.argv.slice(2);
function run(command,argv,env=process.env){const result=spawnSync(command,argv,{cwd:root,env,stdio:'inherit',shell:false});if(result.error)throw result.error;if(result.status!==0)process.exit(result.status||1)}
const prepare=[];for(const key of ['--target']){const i=args.indexOf(key);if(i>=0)prepare.push(key,args[i+1]);}
for(const key of ['--skip-web','--skip-engine'])if(args.includes(key))prepare.push(key);
run(process.execPath,[path.join(root,'scripts','prepare-desktop.mjs'),...prepare]);
const cli=path.join(root,'web','node_modules','@tauri-apps','cli','tauri.js');
if(!existsSync(cli))throw Error('Install web build tools first: npm ci --prefix web');
const command=args.includes('--dev')?'dev':'build';
const cliArgs=[command,'--config',path.join(root,'src-tauri','tauri.conf.json')];
for(const key of ['--target','--bundles']){const i=args.indexOf(key);if(i>=0)cliArgs.push(key,args[i+1]);}
if(args.includes('--debug') && command==='build')cliArgs.push('--debug');
run(process.execPath,[cli,...cliArgs],{...process.env,CARGO_TARGET_DIR:path.join(root,'src-tauri','target')});
