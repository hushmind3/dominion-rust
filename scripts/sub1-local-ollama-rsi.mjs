/**
 * Sub1 local-model RSI: complete real source-edit attempt, not an inference smoke.
 * All revisions are created by DOMINION's real isolated worktree and the live model.
 * A successful workflow only means the attempt finished; quality needs frozen holdouts.
 */
import {mkdirSync,writeFileSync} from "node:fs";
import {dirname,resolve} from "node:path";
import {execFileSync} from "node:child_process";
const host=process.env.DOMINION_LIVE_BASE||"http://127.0.0.1:44327";
const ollama=process.env.DOMINION_LIVE_OLLAMA||"http://127.0.0.1:11434";
const model=process.env.DOMINION_LIVE_MODEL||"qwen3:4b-instruct";
const out=resolve(process.env.DOMINION_LIVE_REPORT||"benchmark/sub1-local-model-rsi.json");
const sha=execFileSync("git",["rev-parse","HEAD"],{encoding:"utf8"}).trim();
const terminal=new Set(["completed","failed","cancelled","needs_review"]);
const report={kind:"dominion.sub1.real_local_model.rsi",source:"DOMINION_RUST_PLUS_OLLAMA",
    sourceCommit:sha,model,startedAt:new Date().toISOString(),
    providerId:null,jobId:null,status:"initializing",history:[],
    summary:{sourceEditCalls:0,candidates:0,verifiedAcceptances:0,modelCalls:0},
    error:null,finishedAt:null};
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
function persist(){mkdirSync(dirname(out),{recursive:true});writeFileSync(out,JSON.stringify(report,null,2)+"\n")}
async function json(url,method="GET",body,timeout=30000){
 const response=await fetch(url,{method,headers:body===undefined?{}:{"content-type":"application/json"},
    body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(timeout)});
 const raw=await response.text();let value;
 try{value=JSON.parse(raw)}catch{value={raw:raw.slice(0,900)}}
 if(!response.ok)throw Error(method+" "+url+" HTTP "+response.status+" "+JSON.stringify(value).slice(0,500));
 return value;
}
async function healthy(){
 for(let i=0;i<70;i++){try{await json(host+"/api/health");return}catch{await sleep(1500)}}
 throw Error("DOMINION Rust local engine did not start");
}
async function run(){
 persist();await healthy();
 const tags=await json(ollama+"/api/tags");
 if(!(tags.models||[]).some(x=>x.model===model||x.name===model))
  throw Error("Actual local model not installed: "+model);
 const provider=await json(host+"/api/providers","POST",{
    name:"Sub1 native Ollama source-repair trial",kind:"ollama",local:true,
    enabled:true,baseUrl:ollama,model,toolMode:"native",contextTokens:16384});
 report.providerId=provider.id;if(!report.providerId)throw Error("Provider not persisted");
 // RSI runs against the explicitly configured detached Git checkout, never
 // against the server binary's build folder or a missing default path.
 const worktree=process.env.DOMINION_WORKSPACE;
 if(!worktree)throw Error("Isolated DOMINION_WORKSPACE not configured");
 await json(host+"/api/dev/config","POST",{sourceDir:worktree});
 // Match the public-run task so differences are in model execution, not a new benchmark.
 const request="Fix a reproducible behavior bug in src/engine/decompose.rs. split_markers() misses Korean action suffix -고 and connectives 다음/나서, causing 설계를 읽고 결과를 저장한 다음 검증해 to stay one step instead of three actions. Read the source excerpt, then actually call workspace_replace with the exact oldText and an implemented Rust change. Avoid splitting nouns like 사고/최고. Preserve subtask text and list handling. If a tool rejects an edit, read the error and correct the tool arguments. Continue to independent frozen baseline/candidate evaluation; reporting ideas without a source edit is not completion.";
 const started=await json(host+"/api/rsi/start","POST",{
    mode:"continuous",confirmation:"START_RSI_EVOLUTION",selectionPolicy:"automatic",
    providerId:report.providerId,model,request,
    focusFiles:["src/engine/decompose.rs"],maxAttempts:3,
    tokenBudget:70000,timeBudgetSeconds:2100},120000);
 report.jobId=started.id;if(!report.jobId)throw Error("RSI campaign job missing");
 report.status="running";persist();
 const deadline=Date.now()+35*60*1000;
 while(Date.now()<deadline) {
  await sleep(10000);
  const result=await json(host+"/api/jobs/"+encodeURIComponent(report.jobId));
  const record=result.evolution||result.result?.evolution||{};
  report.status=result.status;
  report.stopReason=record.stopReason??null;
  report.error=record.error||result.error||null;
  report.history=(record.history||[]).map(g=>({
     attempt:g.attempt,model:g.generatorModel,generatorCommit:g.generatorCommit,
     tokenUsage:g.tokenUsage,focusFiles:g.focusFiles,
     improvementDirective:g.improvementDirective,
     improvementActionObservation:g.improvementActionObservation,
     toolsUsed:g.toolsUsed||[],candidateId:g.candidateId||null,
     accepted:g.accepted===true,acceptedCommit:g.acceptedCommit||null,
     verdict:g.evaluation?.verdict||null,metrics:g.evaluation?.metrics||null,error:g.error||null}));
  const edits=report.history.flatMap(g=>g.toolsUsed).filter(x=>["workspace_replace","workspace_write"].includes(x.tool));
  report.summary={
     modelCalls:report.history.reduce((n,g)=>n+(g.tokenUsage?.calls||0),0),
     sourceEditCalls:edits.reduce((n,x)=>n+(x.calls||0),0),
     candidates:report.history.filter(g=>g.candidateId).length,
     verifiedAcceptances:report.history.filter(g=>g.accepted&&g.verdict==="improved"
       &&g.metrics?.eligible===true&&Number(g.metrics?.improvedHoldouts)>0
       &&Number(g.metrics?.regressions)===0).length,
     providerInterrupted:report.history.some(g=>g.improvementActionObservation?.status==="provider_interrupted")};
  persist();if(terminal.has(report.status))break;
 }
 if(!terminal.has(report.status)){report.status="runner_deadline";report.error="Local model run did not finish within local experiment budget";}
 report.finishedAt=new Date().toISOString();persist();
 console.log(JSON.stringify({status:report.status,summary:report.summary,
   errors:report.history.map(g=>g.error).filter(Boolean),jobId:report.jobId},null,2));
}
try{await run()}catch(e){report.error=String(e);report.status="runner_error";
 report.finishedAt=new Date().toISOString();persist();console.error(e);process.exitCode=1;}
