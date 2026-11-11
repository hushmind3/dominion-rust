/**
 * Champion real Ollama provider smoke.
 * Verifies real local inference through DOMINION's HTTP/streaming engine.
 * Never substitutes a fixture for a model; tool-loop results are reported
 * separately from basic model connectivity.
 */
import {mkdirSync,writeFileSync} from "node:fs";
import {dirname,resolve} from "node:path";
const base=process.env.DOMINION_LIVE_BASE||"http://127.0.0.1:44327";
const ollama=process.env.DOMINION_LIVE_OLLAMA||"http://127.0.0.1:11434";
const model=process.env.DOMINION_LIVE_MODEL||"qwen3:4b-instruct";
const report=resolve(process.env.DOMINION_LIVE_REPORT||"benchmark/champion-ollama-live.json");
const evidence={schemaVersion:1,kind:"dominion.champion.actual_ollama.live",
  model,source:"real_native_engine_and_real_local_model",startedAt:new Date().toISOString(),
  providerId:null,turns:[],transportConnected:false,toolExecutionObserved:false,
  finishedAt:null,error:null};
function persist(){mkdirSync(dirname(report),{recursive:true});writeFileSync(report,JSON.stringify(evidence,null,2)+"\n");}
async function json(path,method="GET",body){
 const res=await fetch(path,{method,headers:body===undefined?{}:{"content-type":"application/json"},
  body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30000)});
 const text=await res.text();let value;
 try{value=JSON.parse(text);}catch{value={raw:text.slice(0,1000)};}
 if(!res.ok)throw Error(method+" "+path+" HTTP "+res.status+": "+JSON.stringify(value).slice(0,500));
 return value;
}
async function healthy(){
 for(let i=0;i<80;i++){try{await json(base+"/api/health");return;}catch{await new Promise(r=>setTimeout(r,1500));}}
 throw Error("DOMINION server did not start");
}
function eventsFromSse(s){
 return s.split(/\r?\n/).filter(line=>line.startsWith("data: "))
  .flatMap(line=>{try{return [JSON.parse(line.slice(6))]}catch{return []}});
}
async function liveTurn(input,toolsEnabled){
 const r=await fetch(base+"/api/chat",{
   method:"POST",headers:{"content-type":"application/json"},
   body:JSON.stringify({input,providerId:evidence.providerId,model,
      toolsEnabled,toolsExecutionEnabled:toolsEnabled,thinking:false,autoLearning:false}),
   signal:AbortSignal.timeout(toolsEnabled?360000:180000)});
 const body=await r.text();
 const events=eventsFromSse(body);
 const errs=events.filter(e=>e.type==="error");
 const done=events.some(e=>e.type==="done");
 const saved=events.some(e=>e.type==="assistant_saved");
 const tools=events.filter(e=>/tool|operation/.test(e.type||"")).map(e=>({type:e.type,tool:e.tool||e.name||null}));
 const turn={goal:toolsEnabled?"actual_tool_loop":"inference_connection",
   status:r.status,done,saved,error:errs.map(e=>e.message),
   eventTypes:[...new Set(events.map(e=>e.type))],tools,
   eventCount:events.length,responseBytes:body.length};
 evidence.turns.push(turn);persist();return turn;
}
async function main(){
 persist();await healthy();
 const tags=await json(ollama+"/api/tags");
 if(!tags.models?.some(x=>x.name===model||x.model===model))throw Error("local Ollama model not installed: "+model);
 const provider=await json(base+"/api/providers","POST",{
   name:"Champion actual Ollama smoke",kind:"ollama",local:true,enabled:true,
   baseUrl:ollama,model,toolMode:"native",contextTokens:16384});
 evidence.providerId=provider.id;persist();
 if(!evidence.providerId)throw Error("DOMINION did not persist Ollama provider");
 const first=await liveTurn("한국어로 준비 완료라고만 답해주세요.",false);
 evidence.transportConnected=first.status===200&&first.done&&first.saved&&first.error.length===0;
 if(!evidence.transportConnected)throw Error("native local-model chat did not complete: "+JSON.stringify(first));
 const second=await liveTurn("현재 작업공간의 최상위 파일 목록을 workspace_list 도구로 실제 조회한 후 한 문장으로 요약해주세요.",true);
 evidence.toolExecutionObserved=second.saved&&second.tools.some(x=>/tool|operation/.test(x.type));
 evidence.finishedAt=new Date().toISOString();persist();
 console.log(JSON.stringify(evidence,null,2));
 if(second.error.length)console.log("Tool loop diagnostic (not basic inference failure): "+second.error.join("; "));
}
try{await main()}catch(err){evidence.error=String(err);evidence.finishedAt=new Date().toISOString();persist();console.error(err);process.exitCode=1;}
