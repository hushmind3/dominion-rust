/**
 * Native DOMINION -> SWE-bench Verified end-to-end runner.
 *
 * Input: official SWE-bench Verified JSONL, local bare/working Git mirrors,
 *        an actual DOMINION Rust executable and a real provider config.
 * Output: official predictions JSONL, original goal/job receipts, every token
 *         report (including failed tasks) and an optional OFFICIAL Docker
 *         evaluation run. No mock prediction, success or test output.
 *
 * Never show test_patch, patch, FAIL_TO_PASS or PASS_TO_PASS to the agent.
 * Each problem has a completely separate, disposable checkout AND database.
 */
import {createHash, randomUUID} from 'node:crypto';
import {existsSync,mkdirSync,readFileSync,writeFileSync,realpathSync} from 'node:fs';
import {createServer} from 'node:net';
import {spawn,spawnSync} from 'node:child_process';
import {dirname,isAbsolute,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const NOW=()=>new Date().toISOString();
const sleep=(ms)=>new Promise(done=>setTimeout(done,ms));
const SHA=(value)=>createHash('sha256').update(value).digest('hex');
const TERMINAL=new Set(['completed','failed','cancelled','needs_review']);
const DISALLOWED=new Set(['test_patch','patch','FAIL_TO_PASS','PASS_TO_PASS','environment_setup_commit','hints_text']);

function demand(ok,reason){if(!ok)throw Error(reason)}
export function taskPublicView(task){
  demand(task && typeof task==='object','task must be object');
  const {instance_id,repo,base_commit,problem_statement}=task;
  demand(typeof instance_id==='string'&&/^[a-zA-Z0-9_.-]{1,160}$/.test(instance_id),'invalid SWE-bench instance id');
  demand(typeof repo==='string'&&/^[\w.-]+\/[\w.-]+$/.test(repo),'invalid repo');
  demand(typeof base_commit==='string'&&/^[a-f0-9]{40}$/.test(base_commit),'base commit must be pinned');
  demand(typeof problem_statement==='string'&&problem_statement.trim().length>0,'missing issue text');
  // The agent must never be given official test patches, reference solution
  // patches, hidden test identifiers or evaluation metadata.
  return {instance_id,repo,base_commit,problem_statement};
}
export function prediction(instance,patch,model){
  demand(typeof patch==='string' && typeof model==='string' && model.length>0,'invalid real prediction');
  return {instance_id:taskPublicView(instance).instance_id,model_patch:patch,model_name_or_path:model};
}
export function summarize(rows){
  const total=rows.length;
  const withPatches=rows.filter(row=>row.patchBytes>0).length;
  const counted=rows.filter(row=>Number.isSafeInteger(row.usage?.totalTokens)&&row.usage.totalTokens>=0);
  const tokens=counted.reduce((sum,row)=>sum+row.usage.totalTokens,0);
  const goalsCompleted=rows.filter(row=>row.goalStatus==='completed').length;
  return {
    attempted:total,
    generatedPatches:withPatches,
    nativeGoalCompleted:goalsCompleted,
    totalRecordedTokens:counted.length===total?tokens:null,
    usageCoverage:total?counted.length/total:null,
    tokensPerGeneratedPatch:counted.length===total&&withPatches?tokens/withPatches:null,
    officialResolved:null,
    officialResolveRate:null,
    note:'Only the official SWE-bench Docker grader can populate officialResolved and officialResolveRate. Native goal completion and nonempty patches are NOT solved tests.'
  };
}
function command(bin,args,options={}){
  const run=spawnSync(bin,args,{encoding:'utf8',timeout:options.timeout||60000,
    maxBuffer:32*1024*1024,env:options.env||process.env,cwd:options.cwd});
  if(run.error||run.status!==0)throw Error((run.stderr||run.error?.message||'command failed').slice(-1800));
  return run.stdout.trimEnd();
}
function cwdGit(dir,args){return command('git',['-C',dir,...args]);}
async function vacantPort(){
  const server=createServer();
  await new Promise((done,fail)=>{server.once('error',fail);server.listen(0,'127.0.0.1',done)});
  const port=server.address().port;
  await new Promise((done,fail)=>server.close(error=>error?fail(error):done()));
  return port;
}
async function request(base,endpoint,method='GET',body,timeout=20000){
  const response=await fetch(base+endpoint,{method,
    headers:body===undefined?{}:{'content-type':'application/json'},
    body:body===undefined?undefined:JSON.stringify(body),
    signal:AbortSignal.timeout(timeout)});
  const text=await response.text();
  let result;try{result=JSON.parse(text)}catch{result={text:text.slice(0,2000)}}
  if(!response.ok)throw Error(endpoint+' '+response.status+' '+JSON.stringify(result).slice(0,1200));
  return result;
}
async function ready(base,child){
  for(let n=0;n<100;n++){
    if(child.exitCode!==null)throw Error('native engine exited before serving: '+child.exitCode);
    try{const result=await request(base,'/api/health','GET',undefined,1500);if(result)return;}
    catch{await sleep(250);}
  }
  throw Error('native Rust engine failed to start in 25s');
}
function readTasks(path){
  const content=readFileSync(path,'utf8');
  const tasks=content.split(/\r?\n/).filter(x=>x.trim()).map(line=>taskPublicView(JSON.parse(line)));
  demand(tasks.length>0,'no SWE-bench tasks');
  const seen=new Set();
  for(const task of tasks){demand(!seen.has(task.instance_id),'duplicate SWE-bench id');seen.add(task.instance_id)}
  return tasks;
}
export function verifyManifest(tasks){
  const checked=tasks.map(taskPublicView);
  const ids=new Set(checked.map(t=>t.instance_id));
  demand(ids.size===checked.length,'duplicate instances');
  return checked;
}
function installCheckout(root,mirrorRoot,task){
  const key=task.repo.replace('/','__');
  const mirror=realpathSync(join(mirrorRoot,key+'.git'));
  const rootReal=realpathSync(mirrorRoot);
  demand(mirror.startsWith(rootReal+'/'),'Git mirror escapes mirror root');
  const dir=join(root,'checkout');
  demand(!existsSync(dir),'checkout already exists: '+dir);
  command('git',['clone','--quiet','--no-hardlinks','--',mirror,dir],{timeout:180000});
  cwdGit(dir,['checkout','--detach',task.base_commit]);
  demand(cwdGit(dir,['rev-parse','HEAD'])===task.base_commit,'checkout base revision mismatch');
  demand(!cwdGit(dir,['status','--porcelain']),'workspace must begin clean');
  return dir;
}
function makePatch(dir,base){
  demand(cwdGit(dir,['rev-parse','HEAD'])===base,'base commit changed by the agent');
  // Include true new source files without staging them for a real commit.
  cwdGit(dir,['add','-N','--','.']);
  const patch=cwdGit(dir,['diff','--binary','--no-ext-diff',base,'--','.']);
  const checks=cwdGit(dir,['diff','--check',base,'--','.']);
  demand(checks==='','generated patch failed git diff --check');
  return patch;
}
function safeName(id){return id.replace(/[^A-Za-z0-9_.-]/g,'_')}
function persist(path,record){mkdirSync(dirname(path),{recursive:true});writeFileSync(path,JSON.stringify(record,null,2)+'\n');}
function envProvider(){
  const path=process.env.DOMINION_SWE_PROVIDER_FILE;
  demand(path&&isAbsolute(path),'DOMINION_SWE_PROVIDER_FILE must be absolute');
  const provider=JSON.parse(readFileSync(path,'utf8'));
  demand(typeof provider.baseUrl==='string'&&typeof provider.model==='string','provider needs baseUrl and model');
  demand(['http:','https:'].includes(new URL(provider.baseUrl).protocol),'provider URL must be HTTP(S)');
  return provider;
}
function settings(){
  demand(process.env.DOMINION_SWE_ENABLE==='1','set DOMINION_SWE_ENABLE=1 to run real model tasks');
  const env=process.env;
  for(const name of ['DOMINION_SWE_TASKS','DOMINION_SWE_MIRRORS','DOMINION_SWE_BIN','DOMINION_SWE_OUTPUT']){
    demand(env[name]&&isAbsolute(env[name]),name+' must be an absolute path');
  }
  for(const name of ['DOMINION_SWE_TASKS','DOMINION_SWE_MIRRORS','DOMINION_SWE_BIN']){
    demand(existsSync(env[name]),name+' does not exist');
  }
  const output=resolve(env.DOMINION_SWE_OUTPUT);
  demand(!existsSync(output),'output must be a new directory: '+output);
  const provider=envProvider();
  const maxIterations=Math.min(64,Math.max(1,Number(env.DOMINION_SWE_ITERATIONS||16)));
  const timeoutMinutes=Math.min(90,Math.max(1,Number(env.DOMINION_SWE_TIMEOUT_MIN||20)));
  const limit=Math.max(1,Number(env.DOMINION_SWE_LIMIT||500));
  demand(Number.isInteger(maxIterations)&&Number.isFinite(timeoutMinutes)&&Number.isInteger(limit),'invalid benchmark limits');
  return {tasks:env.DOMINION_SWE_TASKS,mirrors:env.DOMINION_SWE_MIRRORS,
    binary:env.DOMINION_SWE_BIN,output,provider,maxIterations,timeoutMinutes,limit,
    grade:env.DOMINION_SWE_GRADE==='1'};
}
async function runOne(task,index,settings,model){
  const prefix=join(settings.output,'instances',String(index).padStart(3,'0')+'-'+safeName(task.instance_id));
  mkdirSync(prefix,{recursive:true});
  const result={instance_id:task.instance_id,repo:task.repo,
    base_commit:task.base_commit,model,startedAt:NOW(),goalId:null,jobId:null,
    goalStatus:'not_started',patchSha256:null,patchBytes:0,usage:null,receipt:null,
    error:null,elapsedMs:null,officialSolved:null};
  const began=Date.now();
  let child;
  let workspace;
  try{
    workspace=installCheckout(prefix,settings.mirrors,task);
    const port=await vacantPort(),data=join(prefix,'data'),base='http://127.0.0.1:'+port;
    mkdirSync(data,{recursive:true});
    const env={...process.env,DOMINION_PORT:String(port),DOMINION_DATA:data,
      DOMINION_WORKSPACE:workspace,DOMINION_BENCHMARK:'1'};
    child=spawn(settings.binary,[],{cwd:workspace,env,stdio:'ignore'});
    await ready(base,child);
    const provider=await request(base,'/api/providers','POST',{
      ...settings.provider,id:'swebench-provider',enabled:true,
    });
    const providerId=provider.id||'swebench-provider';
    const objective=task.problem_statement+'\n\nImplement a minimal, correct fix in this checked-out repository. Inspect the source, edit the necessary files, run focused tests, diagnose failures, and repair them. Do not alter benchmark tests or evaluation fixtures.';
    const goal=await request(base,'/api/goals','POST',{
      goal:objective,maxIterations:settings.maxIterations,
      maxReplans:6,replanAfter:2,maxExecutorTokens:80000,
      isolateWorktree:false,executor:{providerId,model:settings.provider.model},
      planner:{providerId,model:settings.provider.model},
      reviewer:{providerId,model:settings.provider.model},
    });
    result.goalId=goal.id;result.jobId=goal.jobId;
    demand(result.goalId&&result.jobId,'goal was not persisted');
    const deadline=Date.now()+settings.timeoutMinutes*60000;
    let snapshot=goal;
    do {
      await sleep(1500);
      snapshot=await request(base,'/api/goals/'+encodeURIComponent(result.goalId));
    }while(Date.now()<deadline&&!TERMINAL.has(snapshot.status));
    result.goalStatus=TERMINAL.has(snapshot.status)?snapshot.status:'timeout';
    if(result.goalStatus==='timeout'){
      try{await request(base,'/api/jobs/'+encodeURIComponent(result.jobId)+'/cancel','POST',{})}
      catch(error){result.error='cancel: '+error.message}
    }
    try{
      const receipt=await request(base,'/api/goals/'+encodeURIComponent(result.goalId)+'/receipt');
      result.receipt={id:receipt.id,jobId:receipt.jobId,verifiedCompletion:receipt.verifiedCompletion,
        actionEvidenceCount:receipt.actionEvidenceCount};
    }catch(error){result.receiptError=String(error.message)}
    if(snapshot.sessionId){
      try{
        const usage=await request(base,'/api/sessions/'+encodeURIComponent(snapshot.sessionId)+'/usage');
        result.usage={totalTokens:usage.totalTokens??null,calls:usage.calls??null};
      }catch(error){result.usageError=String(error.message)}
    }
  }catch(error){result.error=String(error.message).slice(0,2200)}
  finally{
    if(child){child.kill('SIGTERM');await Promise.race([
      new Promise(done=>child.once('exit',done)),sleep(3000)]);if(child.exitCode===null)child.kill('SIGKILL');}
  }
  let patch='';
  if(workspace)try{
    patch=makePatch(workspace,task.base_commit);
    result.patchSha256=SHA(patch);result.patchBytes=Buffer.byteLength(patch);
  }catch(error){result.patchError=String(error.message)}
  result.elapsedMs=Date.now()-began;result.completedAt=NOW();
  persist(join(prefix,'receipt.json'),result);
  return {result,prediction:prediction(task,patch,model)};
}
async function run(){
  const s=settings();
  const tasks=readTasks(s.tasks).slice(0,s.limit);
  mkdirSync(s.output,{recursive:true});
  const runId='dominion-swe-'+Date.now()+'-'+randomUUID().slice(0,8);
  const name='DOMINION-Rust-'+s.provider.model;
  const receipts=[],predictions=[];
  const predictionFile=join(s.output,'predictions.jsonl');
  const report=join(s.output,'report.json');
  const save=()=>{
    writeFileSync(predictionFile,predictions.map(x=>JSON.stringify(x)).join('\n')+(predictions.length?'\n':''));
    persist(report,{runId,createdAt:NOW(),dataset:'princeton-nlp/SWE-bench_Verified',
      model:name,instanceIds:tasks.map(x=>x.instance_id),summary:summarize(receipts),receipts,
      evaluatorCommand:['python','-m','swebench.harness.run_evaluation',
        '--dataset_name','princeton-nlp/SWE-bench_Verified',
        '--predictions_path',predictionFile,'--max_workers','2','--run_id',runId]});
  };
  for(let n=0;n<tasks.length;n++){
    const {result,prediction:pred}=await runOne(tasks[n],n,s,name);
    receipts.push(result);predictions.push(pred);save();
    process.stdout.write(JSON.stringify({task:result.instance_id,status:result.goalStatus,
      bytes:result.patchBytes,tokens:result.usage?.totalTokens??null})+'\n');
  }
  if(s.grade){
    const completed=spawnSync('python',['-m','swebench.harness.run_evaluation',
      '--dataset_name','princeton-nlp/SWE-bench_Verified',
      '--predictions_path',predictionFile,
      '--max_workers','2','--run_id',runId],
      {encoding:'utf8',timeout:12*60*60*1000,maxBuffer:32*1024*1024});
    const exit=completed.status??1;
    writeFileSync(join(s.output,'official-evaluator.log'),
      (completed.stdout||'')+'\n'+(completed.stderr||''));
    persist(join(s.output,'official-evaluator-process.json'),{
      command:'python -m swebench.harness.run_evaluation',
      runId,exitStatus:exit,
      note:'Read official SWE-bench generated per-instance reports; zero process exit does not itself imply a resolved task.'
    });
    if(exit!==0)process.exitCode=1;
  }
  process.stdout.write(JSON.stringify({report,predictionFile,summary:summarize(receipts)})+'\n');
}
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  run().catch(error=>{console.error(String(error.stack||error));process.exitCode=2});
}
