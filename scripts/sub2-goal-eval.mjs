/**
 * Real-model autonomous goal smoke evaluation. This script never invents
 * task success, verifier outcomes, token costs or provider bills.
 * RUN ONLY on an isolated benchmark-mode DOMINION instance and a disposable
 * workspace; API/provider calls can incur charges.
 *
 * Opt-in: DOMINION_EVAL_ENABLE=1 DOMINION_EVAL_BASE=... \
 * DOMINION_EVAL_ROOT=/tmp/... DOMINION_EVAL_PROVIDER_ID=... \
 * DOMINION_EVAL_MODEL=... node scripts/sub2-goal-eval.mjs
 */
import { randomUUID, createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const TERMINAL=new Set(["completed","failed","cancelled","needs_review"]);
const DEFAULT_CASES=[
  {id:"write-single",goal:"{{dir}}/result.txt 파일을 만들고 내용을 정확히 AGENT-OK-1 로 저장하세요.",
   checks:[{type:"file_equals",path:"{{dir}}/result.txt",content:"AGENT-OK-1"}]},
  {id:"write-multiple",goal:"{{dir}}/first.txt와 {{dir}}/second.txt 파일 2개를 만드세요. 첫 파일 내용은 FIRST, 둘째 파일 내용은 SECOND 입니다. 둘 다 실제로 저장하세요.",
   checks:[{type:"file_equals",path:"{{dir}}/first.txt",content:"FIRST"},
      {type:"file_equals",path:"{{dir}}/second.txt",content:"SECOND"}]},
  {id:"modify-existing",goal:"기존 {{dir}}/existing.txt의 OLD-STATE를 NEW-STATE로 교체하고 파일을 저장하세요. 다른 파일을 건드리지 마세요.",
   initial:[{path:"{{dir}}/existing.txt",content:"OLD-STATE"}],
   checks:[{type:"file_equals",path:"{{dir}}/existing.txt",content:"NEW-STATE"}]}
];

const iso=()=>new Date().toISOString();
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function assert(condition,message){if(!condition)throw Error(message);}
function safeRelativePath(root,path){
  assert(typeof path==="string"&&path.length>0&&!isAbsolute(path)&&!path.includes("\\"),
    "unsafe or absolute path in evaluation manifest");
  const parts=path.split("/");
  assert(parts.every(p=>p&&p!=="."&&p!=="..")&&path.startsWith("eval-runs/"),
    "path must stay under the unique evaluation run");
  const full=resolve(root,...parts);
  const rel=relative(root,full);
  assert(rel&&!rel.startsWith(".."+sep)&&!isAbsolute(rel),
    "evaluation file escapes isolated workspace");
  let p=root;
  for(const part of parts){
    p=join(p,part);
    if(existsSync(p)||lstatMaybe(p)){
      assert(!lstatSync(p).isSymbolicLink(),"symlinked evaluation path denied");
    }
  }
  return full;
}
function lstatMaybe(path){try{return lstatSync(path)}catch{return null}}
export function oracle(workspace,checks){
  const rows=checks.map(check=>{
    let passed=false,details="failed";
    try{
      const file=safeRelativePath(workspace,check.path);
      const stat=lstatMaybe(file);
      if(check.type==="file_absent"){
        passed=stat===null;details=passed?"absent":"file exists";
      }else if(stat?.isFile()&&stat.size<=8*1024*1024){
        if(check.type==="file_exists")passed=true;
        else if(check.type==="file_equals"){
          passed=readFileSync(file).equals(Buffer.from(check.content,"utf8"));
        }else if(check.type==="file_contains"){
          passed=readFileSync(file,"utf8").includes(check.contains);
        }
        details=passed?"verified":"content or type mismatch";
      }else{
        details="missing or unsupported artifact";
      }
    }catch(error){details=String(error.message).slice(0,200)}
    return {type:check.type,path:check.path,passed,details};
  });
  return {passed:rows.length>0&&rows.every(x=>x.passed),checks:rows};
}
export function summarize(records){
  const attempts=records.length;
  const verified=records.filter(row=>row.verified).length;
  const measured=records.filter(row=>Number.isFinite(row.usage?.totalTokens));
  const tokens=measured.reduce((sum,row)=>sum+row.usage.totalTokens,0);
  const unknown=attempts-measured.length;
  return {
    attempts,verified,failedOrUnverified:attempts-verified,
    passAt1:attempts?verified/attempts:null,
    recordedTotalTokens:tokens,
    missingUsageReports:unknown,
    recordedTokensPerVerifiedCompletion:verified&&unknown===0?tokens/verified:null,
    actualBilledKRW:null,
    currencyCostReason:"Provider billing amounts, local electricity and GPU costs were not independently observed"
  };
}
function render(value,dir){
  if(typeof value==="string")return value.replaceAll("{{dir}}",dir);
  if(Array.isArray(value))return value.map(x=>render(x,dir));
  if(value&&typeof value==="object"){
    return Object.fromEntries(Object.entries(value).map(([k,v])=>[k,render(v,dir)]));
  }
  return value;
}
async function request(base,path,method="GET",data){
  const resp=await fetch(base+path,{
    method,headers:data?{"content-type":"application/json"}:{},
    body:data?JSON.stringify(data):undefined,signal:AbortSignal.timeout(20000)
  });
  let body;
  try{body=await resp.json()}catch{body={}}
  if(!resp.ok)throw Error(`${method} ${path}: HTTP ${resp.status} ${JSON.stringify(body).slice(0,300)}`);
  return body;
}
function localProvider(provider){
  if(provider?.kind==="cli"||provider?.local===true)return true;
  try{
    const host=new URL(provider.baseUrl).hostname;
    return ["localhost","127.0.0.1","::1","[::1]"].includes(host);
  }catch{return false}
}
function numericEnv(name,fallback,minimum,maximum){
  const value=Number(process.env[name]||fallback);
  assert(Number.isInteger(value)&&value>=minimum&&value<=maximum,
    `invalid ${name}`);
  return value;
}
async function run(){
  assert(process.env.DOMINION_EVAL_ENABLE==="1",
    "Set DOMINION_EVAL_ENABLE=1 explicitly; no provider calls made.");
  const root=process.env.DOMINION_EVAL_ROOT;
  assert(root&&existsSync(root),"DOMINION_EVAL_ROOT must be an existing disposable directory");
  const absolute=realpathSync(root);
  assert(!absolute.includes("/dominion-rust")&&
    readFileSync(join(absolute,".dominion-benchmark-sentinel"),"utf8")
      .toLowerCase().includes("benchmark"),"isolated benchmark sentinel required");
  const workspace=realpathSync(join(absolute,"workspace"));
  const dataRoot=realpathSync(join(absolute,"data"));
  assert(!lstatSync(join(absolute,"workspace")).isSymbolicLink(),
    "benchmark workspace cannot be a symlink");
  const url=process.env.DOMINION_EVAL_BASE;
  assert(url,"DOMINION_EVAL_BASE is required");
  const parsed=new URL(url);
  assert(parsed.protocol==="http:"&&
    ["127.0.0.1","localhost","::1","[::1]"].includes(parsed.hostname),
    "evaluation API must be an isolated loopback HTTP instance");
  const base=parsed.origin;
  const identity=await request(base,"/api/benchmark/identity");
  assert(identity.benchmark===true&&identity.instanceId&&
    identity.dataRoot===dataRoot&&identity.workspace===workspace,
    "engine benchmark identity/data/workspace mismatch; refusing to execute");
  const providerId=process.env.DOMINION_EVAL_PROVIDER_ID;
  const model=process.env.DOMINION_EVAL_MODEL;
  assert(providerId&&model,"explicit provider ID and model are required");
  const providers=await request(base,"/api/providers");
  const provider=providers.find(p=>p.id===providerId);
  assert(provider&&provider.enabled!==false,"provider not found or disabled");
  if(!localProvider(provider)){
    assert(process.env.DOMINION_EVAL_ALLOW_PAID==="1",
      "remote provider may incur real charges: explicit paid opt-in required");
  }
  const maxIterations=numericEnv("DOMINION_EVAL_MAX_ITERATIONS",6,1,32);
  const maxTokens=numericEnv("DOMINION_EVAL_MAX_TOKENS",12000,5000,2000000);
  const maxMinutes=numericEnv("DOMINION_EVAL_MAX_MINUTES",15,1,120);
  const rootId=randomUUID();
  const manifest=process.env.DOMINION_EVAL_CASES?
    JSON.parse(readFileSync(process.env.DOMINION_EVAL_CASES,"utf8")):DEFAULT_CASES;
  assert(Array.isArray(manifest)&&manifest.length>=1&&manifest.length<=10,
    "1-10 immutable task definitions expected");
  const reportDir=join(absolute,"results");
  mkdirSync(reportDir,{recursive:true});
  const output=join(reportDir,`goal-eval-${rootId}.json`);
  let commit=null;
  try{commit=execFileSync("git",["rev-parse","HEAD"],{encoding:"utf8"}).trim()}catch{}
  const record={schema:"dominion.sub2.goal.live-eval.v1",
    createdAt:iso(),head:commit,runnerRunId:rootId,
    engineInstanceId:identity.instanceId,providerId,model,
    parameters:{maxIterations,maxTokens,maxMinutes},
    verifiedModelRun:true,paidProviderAllowed:!localProvider(provider),
    cases:[],summary:null,completedAt:null};
  const persist=()=>{
    record.summary=summarize(record.cases);
    writeFileSync(output,JSON.stringify(record,null,2)+"\n");
  };
  persist();
  for(const task of manifest){
    assert(task.id&&/^[a-z0-9-]{1,48}$/.test(task.id),"invalid case ID");
    assert(typeof task.goal==="string"&&Array.isArray(task.checks)&&
      task.checks.length>0&&task.checks.length<=10,"invalid case definition");
    const dir=`eval-runs/${rootId}/${task.id}`;
    const goal=render(task.goal,dir),checks=render(task.checks,dir);
    for(const check of checks) {
      assert(["file_exists","file_absent","file_equals","file_contains"].includes(check.type),
        "unsupported independent oracle check");
      assert(typeof check.path==="string"&&check.path.startsWith(dir+"/"),
        "manifest cannot write outside its own case");
      if(check.type==="file_equals")assert(typeof check.content==="string","missing expected content");
      if(check.type==="file_contains")assert(typeof check.contains==="string","missing expected substring");
      safeRelativePath(workspace,check.path);
    }
    for(const initial of render(task.initial||[],dir)){
      assert(initial.path.startsWith(dir+"/")&&typeof initial.content==="string",
        "invalid seed file");
      const filename=safeRelativePath(workspace,initial.path);
      assert(!existsSync(filename),"evaluation seed already exists");
      mkdirSync(dirname(filename),{recursive:true});
      writeFileSync(filename,initial.content,"utf8");
    }
    const row={id:task.id,at:iso(),status:"not_started",jobId:null,goalId:null,
      verified:false,independentOracle:null,receipt:null,usage:null,
      durationMs:null,error:null};
    record.cases.push(row);persist();
    const began=Date.now();
    try{
      const accepted=await request(base,"/api/goals","POST",{
        goal,successCriteria:checks,executor:{providerId,model},
        maxIterations,maxExecutorTokens:maxTokens,
        isolateWorktree:false
      });
      row.goalId=accepted.id;row.jobId=accepted.jobId;
      assert(row.goalId&&row.jobId,"goal was not durably admitted");
      const end=Date.now()+maxMinutes*60*1000;
      let current=accepted;
      while(Date.now()<end){
        current=await request(base,`/api/goals/${row.goalId}`);
        if(TERMINAL.has(current.status))break;
        await wait(1500);
      }
      if(!TERMINAL.has(current.status)){
        row.status="timeout";
        try{await request(base,`/api/jobs/${row.jobId}/cancel`,"POST",{})}
        catch(error){row.error=String(error.message)}
      }else{row.status=current.status}
      row.independentOracle=oracle(workspace,checks);
      try{row.receipt=await request(base,`/api/goals/${row.goalId}/receipt`)}
      catch(error){row.error=`receipt: ${error.message}`;}
      if(current.sessionId){
        try{row.usage=await request(base,`/api/sessions/${current.sessionId}/usage`)}
        catch(error){row.error=`usage: ${error.message}`;}
      }
      row.verified=row.status==="completed"&&
        row.receipt?.jobId===row.jobId&&
        row.receipt?.verifiedCompletion===true&&
        row.receipt?.actionEvidenceCount>0&&
        row.independentOracle.passed;
    }catch(error){row.status="error";row.error=String(error.message);}
    row.durationMs=Date.now()-began;
    persist();
  }
  record.completedAt=iso();persist();
  console.log(JSON.stringify({report:output,summary:record.summary},null,2));
  if(record.summary.verified!==record.summary.attempts)process.exitCode=1;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  run().catch(error=>{console.error(String(error));process.exitCode=2});
}
