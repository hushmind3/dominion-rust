/**
 * Official GitHub Copilot SDK baseline against the exact same manifest/oracle
 * as Dominion's native goal evaluator. NOT a simulated benchmark or score.
 *
 * npm ci --prefix scripts/copilot-sdk-baseline
 * DOMINION_EVAL_ENABLE=1 DOMINION_EVAL_ROOT=/tmp/dominion-benchmark \
 * DOMINION_SDK_MODEL=qwen3:8b DOMINION_SDK_BASE_URL=http://127.0.0.1:11434/v1 \
 * node scripts/copilot-sdk-baseline/runner.mjs
 *
 * Root MUST contain .dominion-benchmark-sentinel with 'benchmark', plus
 * existing workspace/ and data/ dirs. Run Dominion's native comparator on
 * the same root / model / provider for meaningful paired analysis.
 */
import {randomUUID} from "node:crypto";
import {
  existsSync,lstatSync,readFileSync,realpathSync,mkdirSync,writeFileSync
} from "node:fs";
import {execFileSync} from "node:child_process";
import {dirname,isAbsolute,join,relative,resolve,sep} from "node:path";
import {fileURLToPath} from "node:url";
import {DEFAULT_CASES,oracle,render,safeRelativePath} from "../sub2-goal-eval.mjs";

const iso=()=>new Date().toISOString();
const assert=(value,message)=>{if(!value)throw Error(message)};
const num=(name,fallback,min,max)=>{
  const v=Number(process.env[name]||fallback);
  assert(Number.isInteger(v)&&v>=min&&v<=max,`invalid ${name}`);
  return v;
};
export function isLocalHost(url){
  try{
    const u=new URL(url);
    return u.protocol==="http:" &&
      ["127.0.0.1","localhost","::1","[::1]"].includes(u.hostname) &&
      u.username===""&&u.password==="";
  }catch{return false}
}
// A permission callback is NOT an OS sandbox; the benchmark runner uses an
// expendable workspace, refuses shells/MCP/web and confines file paths.
function lstatMaybe(path){
  try{return lstatSync(path)}catch(e){if(e.code==="ENOENT")return null;throw e}
}
export function withinCase(workspace,caseDir,fileName){
  if(typeof fileName!=="string"||fileName.length===0||fileName.includes("\\")||
     fileName.includes("\0"))return false;
  const absolute=isAbsolute(fileName)?resolve(fileName):resolve(workspace,fileName);
  const inside=relative(caseDir,absolute);
  if(inside.startsWith(".."+sep)||inside===".."||isAbsolute(inside)||!inside)return false;
  // Reject symlink components, including dangling symlinks and any ancestor
  // already accessible outside the benchmark tree.
  let current=caseDir;
  for(const part of inside.split(sep)){
    current=join(current,part);
    const info=lstatMaybe(current);
    if(info?.isSymbolicLink())return false;
  }
  return true;
}
export function permissionForCase(workspace,caseDir,request){
  if(request?.managedApprovalRequired===true)
    return {kind:"reject",feedback:"Managed policies require human review."};
  const type=request?.kind;
  if(type!=="read"&&type!=="write")
    return {kind:"reject",feedback:"Only read/write in the isolated case are enabled."};
  const name=request?.fileName;
  if(!withinCase(workspace,caseDir,name))
    return {kind:"reject",feedback:"File request is not within the isolated benchmark case."};
  return {kind:"approve-once"};
}
export function validateBenchmarkRoot(root){
  assert(root&&existsSync(root),"DOMINION_EVAL_ROOT must exist");
  const absolute=realpathSync(root);
  assert(!absolute.includes("/dominion-rust")&&
    readFileSync(join(absolute,".dominion-benchmark-sentinel"),"utf8")
      .toLowerCase().includes("benchmark"),
    "explicit disposable benchmark sentinel required");
  const ws=join(absolute,"workspace"),data=join(absolute,"data");
  assert(!lstatSync(ws).isSymbolicLink()&&!lstatSync(data).isSymbolicLink(),
    "benchmark workspace/data cannot be symlinks");
  assert(lstatSync(ws).isDirectory()&&lstatSync(data).isDirectory(),
    "benchmark workspace/data must be directories");
  return {root:absolute,workspace:realpathSync(ws),data:realpathSync(data)};
}
export function acceptanceProof(row){
  return row.status==="finished"&&row.oracle?.passed===true &&
    Number(row.usage?.calls)>0 && row.error===null &&
    row.limitReached!==true;
}
export function summarizeSdk(rows){
  const verified=rows.filter(r=>r.verified).length;
  const complete=rows.every(r=>r.usage&&
    Number.isFinite(r.usage.totalTokens)&&r.usage.calls>0);
  const total=rows.reduce((s,r)=>s+(Number.isFinite(r.usage?.totalTokens)?
    r.usage.totalTokens:0),0);
  return {attempts:rows.length,verified,failedOrUnverified:rows.length-verified,
    passAt1:rows.length?verified/rows.length:null,
    recordedTotalTokens:total,
    missingUsageReports:rows.filter(r=>!r.usage||r.usage.calls<=0).length,
    recordedTokensPerVerifiedCompletion:verified&&complete?total/verified:null,
    actualBilledKRW:null,providerBillingVerified:false,
    note:"A public SDK is free to install, but inference, premium requests, and electricity may cost money."
  };
}
export async function run(){
  assert(process.env.DOMINION_EVAL_ENABLE==="1",
    "Set DOMINION_EVAL_ENABLE=1. No SDK or model started.");
  const roots=validateBenchmarkRoot(process.env.DOMINION_EVAL_ROOT);
  const model=process.env.DOMINION_SDK_MODEL;
  assert(typeof model==="string"&&model.trim(),"DOMINION_SDK_MODEL is required");
  const baseUrl=process.env.DOMINION_SDK_BASE_URL;
  assert(baseUrl||process.env.DOMINION_EVAL_ALLOW_PAID==="1",
    "Copilot-account or paid model routing needs DOMINION_EVAL_ALLOW_PAID=1");
  if(baseUrl&&!isLocalHost(baseUrl))
    assert(process.env.DOMINION_EVAL_ALLOW_PAID==="1",
      "Remote inference requires DOMINION_EVAL_ALLOW_PAID=1");
  const maxMinutes=num("DOMINION_EVAL_MAX_MINUTES",15,1,120);
  const maxTokens=num("DOMINION_EVAL_MAX_TOKENS",12000,5000,2000000);
  const maxToolCalls=num("DOMINION_SDK_MAX_TOOL_CALLS",40,1,200);
  const manifest=process.env.DOMINION_EVAL_CASES?
    JSON.parse(readFileSync(process.env.DOMINION_EVAL_CASES,"utf8")):DEFAULT_CASES;
  assert(Array.isArray(manifest)&&manifest.length>0&&manifest.length<=10,
    "expected 1-10 comparable tasks");
  const runId=randomUUID();
  mkdirSync(join(roots.root,"results"),{recursive:true});
  let gitCommit=null;
  try{gitCommit=execFileSync("git",["rev-parse","HEAD"],
      {encoding:"utf8"}).trim()}catch{}
  const report={schema:"dominion.copilot-sdk.independent-baseline.v1",
    createdAt:iso(),completedAt:null,runId,head:gitCommit,
    sdkPackage:"@github/copilot-sdk",model,
    providerMode:baseUrl?"byok":"copilot",
    providerBaseUrl:baseUrl||null,
    perCaseTimeLimitMinutes:maxMinutes,tokenSoftLimit:maxTokens,
    toolCallLimit:maxToolCalls,
    pairedWith:"scripts/sub2-goal-eval.mjs",
    cases:[],summary:null};
  const reportFile=join(roots.root,"results",`copilot-sdk-baseline-${runId}.json`);
  const persist=()=>{
    report.summary=summarizeSdk(report.cases);
    writeFileSync(reportFile,JSON.stringify(report,null,2)+"\n");
  };
  persist();
  // Dynamic load after all opt-in and billing checks. Dependency is confined
  // to this benchmark tool, never the shipped app or its UI bundle.
  const {CopilotClient}=await import("@github/copilot-sdk");
  const client=new CopilotClient({workingDirectory:roots.workspace,
    disableClientTelemetry:true});
  try{
    await client.start();
    for(const task of manifest){
      assert(task&&/^[a-z0-9-]{1,48}$/.test(task.id),"invalid case ID");
      assert(typeof task.goal==="string"&&Array.isArray(task.checks)&&
        task.checks.length>0&&task.checks.length<=10,"invalid case");
      const caseRel=`eval-runs/${runId}/${task.id}`;
      const caseDir=safeRelativePath(roots.workspace,caseRel);
      mkdirSync(caseDir,{recursive:true});
      const checks=render(task.checks,caseRel);
      for(const check of checks){
        assert(["file_exists","file_absent","file_equals","file_contains"].includes(check.type),
          "unknown oracle check");
        assert(typeof check.path==="string"&&check.path.startsWith(caseRel+"/"),
          "case tries to inspect outside its own directory");
        safeRelativePath(roots.workspace,check.path);
        if(check.type==="file_equals")assert(typeof check.content==="string",
          "expected content missing");
        if(check.type==="file_contains")assert(typeof check.contains==="string",
          "substring missing");
      }
      for(const seed of render(task.initial||[],caseRel)){
        assert(typeof seed.content==="string"&&seed.path.startsWith(caseRel+"/"),
          "invalid seed");
        const file=safeRelativePath(roots.workspace,seed.path);
        assert(!existsSync(file),"benchmark seed already exists");
        mkdirSync(dirname(file),{recursive:true});
        writeFileSync(file,seed.content);
      }
      const row={id:task.id,status:"not_started",at:iso(),verified:false,
        oracle:null,usage:{calls:0,inputTokens:0,outputTokens:0,totalTokens:0,
          cacheReadTokens:0,cacheWriteTokens:0},
        toolCalls:0,successfulTools:0,deniedPermissions:0,
        limitReached:false,error:null,durationMs:null};
      report.cases.push(row);persist();
      const began=Date.now();
      let session;
      try{
        const provider=baseUrl?{
          type:"openai",baseUrl,wireApi:"completions",
          ...(process.env.DOMINION_SDK_API_KEY?
            {apiKey:process.env.DOMINION_SDK_API_KEY}:{})
        }:undefined;
        session=await client.createSession({
          model,workingDirectory:roots.workspace,
          enableSessionTelemetry:false,
          // Never allow arbitrary bash, web, custom tools or MCP when
          // evaluating a read/write artifact task.
          availableTools:["view","create","edit","grep"],
          includedBuiltinAgents:[],includedBuiltinSkills:[],
          ...(provider?{provider}:{}),
          onPermissionRequest:request=>{
            const decision=permissionForCase(roots.workspace,caseDir,request);
            if(decision.kind!=="approve-once")row.deniedPermissions++;
            return decision;
          }
        });
        session.on("assistant.usage",event=>{
          const d=event.data||{};
          row.usage.calls++;
          for(const key of ["inputTokens","outputTokens","cacheReadTokens","cacheWriteTokens"]){
            if(Number.isFinite(d[key])&&d[key]>=0)row.usage[key]+=d[key];
          }
          row.usage.totalTokens=row.usage.inputTokens+row.usage.outputTokens;
          if(row.usage.totalTokens>maxTokens){
            row.limitReached=true;
            void session.abort().catch(()=>{});
          }
        });
        session.on("tool.execution_start",()=>{
          row.toolCalls++;
          if(row.toolCalls>maxToolCalls){
            row.limitReached=true;
            void session.abort().catch(()=>{});
          }
        });
        session.on("tool.execution_complete",event=>{
          if(event.data?.success===true)row.successfulTools++;
        });
        const prompt=render(task.goal,caseRel)+
          "\nUse only the enabled filesystem tools. No shell, HTTP or other task directories. "+
          "Save actual files to disk; don't claim success based on prose.";
        const reply=await session.sendAndWait(
          {prompt,timeout:maxMinutes*60*1000},maxMinutes*60*1000);
        row.status=reply?"finished":"no_reply";
      }catch(error){
        row.status="error";row.error=String(error).slice(0,800);
      }finally{
        if(session)try{await session.disconnect()}catch{}
        row.oracle=oracle(roots.workspace,checks);
        row.durationMs=Date.now()-began;
        row.verified=acceptanceProof(row);
        persist();
      }
    }
  }finally{
    await client.stop().catch(()=>{});
    report.completedAt=iso();persist();
  }
  console.log(JSON.stringify({report:reportFile,summary:report.summary},null,2));
  if(report.summary.verified!==report.summary.attempts)process.exitCode=1;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  run().catch(error=>{console.error(String(error));process.exitCode=2;});
}
