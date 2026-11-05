// Build-time tooling only. The installed app runs its Rust host + Rust engine.
import {spawnSync} from 'node:child_process';
import {cpSync,existsSync,mkdirSync,copyFileSync,chmodSync,rmSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const args=process.argv.slice(2);
const value=key=>{const i=args.indexOf(key);return i<0?undefined:args[i+1]};
function run(command,argv,cwd=root){const result=spawnSync(command,argv,{cwd,stdio:'inherit',shell:false});if(result.error)throw result.error;if(result.status!==0)process.exit(result.status||1)}
const target=value('--target')||process.env.TAURI_ENV_TARGET_TRIPLE||spawnSync('rustc',['--print','host-tuple'],{encoding:'utf8'}).stdout.trim();
if(!/^[a-zA-Z0-9_-]+$/.test(target))throw Error('A valid Rust target triple is required');
const windows=target.includes('windows');const suffix=windows?'.exe':'';
if(!args.includes('--skip-web')){if(process.platform==='win32')run(process.env.ComSpec||'cmd.exe',['/d','/s','/c','npm run build'],path.join(root,'web'));else run('npm',['run','build'],path.join(root,'web'));}
if(!args.includes('--skip-engine'))run('cargo',['build','--locked','--release','--manifest-path',path.join(root,'Cargo.toml'),'--target',target,'--target-dir',path.join(root,'target','desktop-engine')]);
const built=path.join(root,'target','desktop-engine',target,'release','dominion-rust'+suffix);
const sidecar=path.join(root,'src-tauri','binaries','dominion-engine-'+target+suffix);
mkdirSync(path.dirname(sidecar),{recursive:true});
if(!args.includes('--skip-engine'))copyFileSync(built,sidecar);
if(!existsSync(sidecar))throw Error('Bundled Rust engine is missing; prepare without --skip-engine');
if(!windows)chmodSync(sidecar,0o755);
for(const [source,destination] of [['web/dist','web'],['assets','assets']]){
 const from=path.join(root,source),to=path.join(root,'src-tauri','resources',destination);
 if(!existsSync(from))throw Error('Missing build resource: '+source);
 rmSync(to,{recursive:true,force:true});cpSync(from,to,{recursive:true});
}
console.log('Desktop resources ready for '+target);
