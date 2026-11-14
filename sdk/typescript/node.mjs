// First-party Node process host for the independent Dominion Rust runtime.
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,randomUUID} from 'node:crypto';
import {mkdir,realpath,stat} from 'node:fs/promises';
import {resolve} from 'node:path';
import {DominionClient} from './dist/index.js';
const sleep=ms=>new Promise(done=>setTimeout(done,ms));
async function pickPort(){
  const server=createServer();
  await new Promise((done,fail)=>{
    server.once('error',fail);server.listen(0,'127.0.0.1',done);
  });
  const port=server.address().port;
  await new Promise((done,fail)=>server.close(e=>e?fail(e):done()));
  return port;
}
async function stop(child){
  if(child.exitCode!==null)return;
  const ended=new Promise(done=>child.once('exit',done));
  child.stdin?.end();
  await Promise.race([ended,sleep(5000)]);
  if(child.exitCode===null){
    child.kill('SIGTERM');
    await Promise.race([ended,sleep(1000)]);
    if(child.exitCode===null){
      child.kill('SIGKILL');
      await Promise.race([ended,sleep(1000)]);
    }
  }
}
export class DominionRuntime {
  #child;#client;#instanceId;#port;
  constructor(child,client,id,port){
    this.#child=child;this.#client=client;this.#instanceId=id;this.#port=port;
  }
  get client(){return this.#client}
  get instanceId(){return this.#instanceId}
  get port(){return this.#port}
  static async spawn(config){
    const {binary,workspace,dataDir,webDir,port,startupTimeoutMs=30000}=config||{};
    if(!binary||!workspace||!dataDir)throw new TypeError(
      'binary, workspace, dataDir must be configured');
    if(!Number.isSafeInteger(startupTimeoutMs)||startupTimeoutMs<1000)
      throw new RangeError('invalid startupTimeoutMs');
    if(port!==undefined&&(!Number.isInteger(port)||port<1||port>65535))
      throw new RangeError('invalid native runtime port');
    const executable=await realpath(binary);
    if(!(await stat(executable)).isFile())throw new TypeError('binary not a file');
    const source=await realpath(workspace);
    if(!(await stat(source)).isDirectory())throw new TypeError('workspace not a directory');
    await mkdir(dataDir,{recursive:true});
    const database=await realpath(dataDir);
    const selectedPort=port||await pickPort();
    const instanceId=randomUUID(),adminToken=randomBytes(32).toString('hex');
    const base='http://127.0.0.1:'+selectedPort;
    const child=spawn(executable,[],{
      cwd:source,stdio:['pipe','ignore','inherit'],
      env:{...process.env,DOMINION_PORT:String(selectedPort),
        DOMINION_DATA:database,DOMINION_WORKSPACE:source,
        DOMINION_ENGINE_INSTANCE:instanceId,DOMINION_DESKTOP_PARENT:'1',
        DOMINION_ADMIN_TOKEN:adminToken,
        ...(webDir?{DOMINION_WEB:resolve(webDir)}:{})}
    });
    let spawnError=null;
    child.once('error',e=>{spawnError=e});
    const deadline=Date.now()+startupTimeoutMs;
    try{
      while(Date.now()<deadline){
        if(spawnError)throw spawnError;
        if(child.exitCode!==null)throw new Error('Dominion exited before readiness');
        try{
          const response=await fetch(base+'/api/benchmark/identity',{
            signal:AbortSignal.timeout(800)});
          if(response.ok){
            const identity=await response.json();
            if(identity.instanceId!==instanceId||identity.pid!==child.pid
              ||resolve(identity.workspace||'')!==source
              ||resolve(identity.dataRoot||'')!==database)
              throw new Error('Dominion native runtime identity mismatch');
            return new DominionRuntime(child,
              new DominionClient({baseUrl:base,adminToken}),instanceId,selectedPort);
          }
        }catch(error){
          if(String(error?.message||error).includes('identity mismatch'))throw error;
        }
        await sleep(150);
      }
      throw new Error('Dominion readiness timeout');
    }catch(error){await stop(child);throw error}
  }
  async shutdown(){
    const child=this.#child;this.#child=null;
    if(child)await stop(child);
  }
}
