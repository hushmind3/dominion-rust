/**
 * Sub1 live-model goal benchmark. Calls the ACTUAL HTTP goal worker with a
 * real model, judges changed workspace bytes from the host (not model claims),
 * captures server-audited tokens/cache and writes immutable JSON evidence.
 *
 * Run only against a disposable DOMINION_BENCHMARK=1 instance.
 * No remote billable model is used in the provided workflow.
 *
 * Success of this PROGRAM means measurements were recorded. It does not mean
 * the agent passed the tasks. Read summary.oraclePasses / summary.totalCases.
 */
import {createHash} from "node:crypto";
import {readFileSync,writeFileSync,mkdirSync,existsSync,realpathSync} from "node:fs";
import {dirname,join,resolve} from "node:path";
import {pathToFileURL} from "node:url";
import {appendFileSync} from "node:fs";

const root=resolve(process.env.BENCH_ROOT||"");
const base=process.env.DOMINION_BASE||"";
const model=process.env.BENCH_MODEL||"qwen3:4b-instruct";
const providerId=process.env.BENCH_PROVIDER_ID||"sub1-local-eval";
const reportPath=resolve(process.env.BENCH_REPORT||join(root,"live-goals.json"));
const perTaskMs=Number(process.env.BENCH_TASK_TIMEOUT_MS||600_000);
const sha=x=>createHash("sha256").update(x).digest("hex");
const wait=ms=>new Promise(r=>setTimeout(r,ms));
const read=relative=>readFileSync(join(root,"workspace",relative),"utf8");
const jsonFile=relative=>JSON.parse(read(relative));
function write(relative,body){
 const target=join(root,"workspace",relative);
 mkdirSync(dirname(target),{recursive:true});
 writeFileSync(target,body);
}
async function request(endpoint,body){
 const res=await fetch(base+endpoint,{
  method:body===undefined?"GET":"POST",
  headers:body===undefined?{}:{"content-type":"application/json"},
  body:body===undefined?undefined:JSON.stringify(body),
  signal:AbortSignal.timeout(20000)
 });
 const text=await res.text();
 let data;
 try{data=JSON.parse(text)}catch{data={body:text.slice(0,800)}}
 if(!res.ok)throw Error(endpoint+" "+res.status+" "+JSON.stringify(data).slice(0,650));
 return data;
}
function checkEnvironment(){
 if(!process.env.BENCH_ROOT||!base.startsWith("http://127.0.0.1:"))
  throw Error("BENCH_ROOT and loopback-only DOMINION_BASE are mandatory");
 if(!existsSync(join(root,".dominion-benchmark-sentinel")))
  throw Error("Missing disposable benchmark sentinel");
 const real=realpathSync(root);
 if(real==="/"||real===process.env.HOME||real===resolve(process.cwd()))
  throw Error("Refusing production or repository root");
 for(const dir of ["workspace","data"]){
  if(!existsSync(join(root,dir)))throw Error("Missing isolated "+dir);
  if(realpathSync(join(root,dir))!==join(real,dir))
   throw Error("Symlinked or escaped benchmark "+dir);
 }
 if(!Number.isInteger(perTaskMs)||perTaskMs<10000||perTaskMs>1800000)
  throw Error("Invalid task timeout");
}
const cases=[
 {
  id:"locale-three-files",
  description:"Edit config and two locale files without losing existing keys",
  seed(){
   write("trial/locale/settings.json",'{"theme":"light","locale":"en","version":3}\n');
   write("trial/locale/i18n/en.json",'{"title":"Project","submit":"Submit","cancel":"Cancel"}\n');
   write("trial/locale/i18n/ko.json",'{"title":"프로젝트","submit":"제출","cancel":"취소"}\n');
  },
  goal:"In trial/locale/settings.json change ONLY locale to ko while keeping theme and version. In trial/locale/i18n/en.json add a save key equal to Save, and in trial/locale/i18n/ko.json add a save key equal to 저장. Keep every existing translation key and value. Create trial/locale/DONE.md with one line: locale-ready.",
  successCriteria:[
   {type:"file_contains",path:"trial/locale/settings.json",contains:'"ko"'},
   {type:"file_exists",path:"trial/locale/DONE.md"},
   {type:"file_contains",path:"trial/locale/i18n/ko.json",contains:"저장"}
  ],
  async oracle(){
   const cfg=jsonFile("trial/locale/settings.json");
   const en=jsonFile("trial/locale/i18n/en.json");
   const ko=jsonFile("trial/locale/i18n/ko.json");
   const done=read("trial/locale/DONE.md");
   return cfg.theme==="light"&&cfg.locale==="ko"&&cfg.version===3
    &&Object.keys(cfg).length===3
    &&en.title==="Project"&&en.submit==="Submit"&&en.cancel==="Cancel"&&en.save==="Save"
    &&ko.title==="프로젝트"&&ko.submit==="제출"&&ko.cancel==="취소"&&ko.save==="저장"
    &&done.trim()==="locale-ready";
  },
  output:["trial/locale/settings.json","trial/locale/i18n/en.json","trial/locale/i18n/ko.json","trial/locale/DONE.md"]
 },
 {
  id:"code-edge-cases",
  description:"Correct real executable JS logic and preserve exports",
  seed(){
   write("trial/code/lib/price.mjs", "export function totalWithTax(price, rate) {\n  return price + rate;\n}\nexport const currency = 'KRW';\n");
   write("trial/code/README.md","The price module accepts nonnegative finite price and rate; invalid values return null.\n");
  },
  goal:"Fix trial/code/lib/price.mjs: totalWithTax(price, rate) should return Math.round(price * (1 + rate)) for finite nonnegative numeric price and rate, and return null for invalid, negative or non-finite inputs. Preserve the named currency export exactly as KRW, keep the module ES module compatible, and create trial/code/FIXED.txt containing exactly tax-calculation-fixed.",
  successCriteria:[
   {type:"file_exists",path:"trial/code/FIXED.txt"},
   {type:"file_contains",path:"trial/code/lib/price.mjs",contains:"Math.round"},
  ],
  async oracle(){
   const url=pathToFileURL(join(root,"workspace","trial/code/lib/price.mjs")).href;
   const {totalWithTax,currency}=await import(url+"?sha="+Date.now());
   if(typeof totalWithTax!=="function"||currency!=="KRW")return false;
   const values=[
    [[100,0.1],110],[[100.45,0.05],105],[[0,0],0],
    [[-1,0.1],null],[[100,-0.1],null],[[Infinity,0.2],null],
    [[NaN,0.1],null],[["100",0.1],null],[[100,null],null]
   ];
   return values.every(([args,want])=>Object.is(totalWithTax(...args),want))
    &&read("trial/code/FIXED.txt").trim()==="tax-calculation-fixed"
    &&read("trial/code/README.md")==="The price module accepts nonnegative finite price and rate; invalid values return null.\n";
  },
  output:["trial/code/lib/price.mjs","trial/code/FIXED.txt"]
 }
];
async function launchCase(test){
 test.seed();
 const start=new Date();
 const inputHashes=Object.fromEntries(test.output.filter(p=>existsSync(join(root,"workspace",p)))
  .map(p=>[p,sha(read(p))]));
 let goal=null,error=null,job=null;
 try{
  const submitted=await request("/api/goals",{
   goal:test.goal,successCriteria:test.successCriteria,
   executor:{providerId,model},planner:{providerId,model},reviewer:{providerId,model},
   maxIterations:6,maxReplans:1,replanAfter:2,tokenBudget:28000,
   isolateWorktree:false
  });
  if(!submitted.id||!submitted.jobId)throw Error("No durable goal/job ID");
  const deadline=Date.now()+perTaskMs;
  while(Date.now()<deadline){
   await wait(2500);
   goal=await request("/api/goals/"+encodeURIComponent(submitted.id));
   if(["completed","failed","needs_review","cancelled"].includes(goal.status))break;
  }
  if(!goal||!["completed","failed","needs_review","cancelled"].includes(goal.status)){
   error="time_budget_exhausted";
   await request("/api/goals/"+encodeURIComponent(submitted.id)+"/cancel",{}).catch(()=>{});
   goal=await request("/api/goals/"+encodeURIComponent(submitted.id)).catch(()=>goal);
  }
  job=await request("/api/jobs/"+encodeURIComponent(submitted.jobId)).catch(()=>null);
 }catch(e){error=String(e)}
 let oraclePass=false,oracleError=null;
 try{oraclePass=await test.oracle()}catch(e){oracleError=String(e)}
 const outputHashes=Object.fromEntries(test.output.filter(p=>existsSync(join(root,"workspace",p)))
  .map(p=>[p,sha(read(p))]));
 return {
  caseId:test.id,description:test.description,goalId:goal?.id??null,
  jobId:goal?.jobId??null,status:goal?.status??"not_registered",
  engineVerified:goal?.metrics?.verifiedCompleted===true,
  oraclePass,oracleError,
  startedAt:start.toISOString(),endedAt:new Date().toISOString(),
  elapsedMs:Date.now()-start.getTime(),
  inputHashes,outputHashes,
  auditSource:goal?.metrics?.source??null,
  usage:goal?.metrics?.usage??null,
  evidenceCount:goal?.metrics?.successfulToolEvidenceCount??0,
  unmetGaps:goal?.gaps?.length??null,
  taskError:error,jobStatus:job?.status??null,
  // Neither model text nor source-side success flags are an oracle.
  evidenceModelClaimTrusted:false,
 };
}
const receipt={schemaVersion:1,kind:"sub1.real_model.goal_task_outcomes",
 source:"isolated_live_dominion_goal_jobs_with_private_host_oracle",
 revision:process.env.GITHUB_SHA||"unversioned",model,providerId,
 startedAt:new Date().toISOString(),environment:null,
 cases:[],summary:null,actualBillingKrw:null,
 caveat:"Real local model + real tools, two small deterministic tasks. Not large-repository SWE-bench or general task completion."};
const persist=()=>{
 mkdirSync(dirname(reportPath),{recursive:true});
 writeFileSync(reportPath,JSON.stringify(receipt,null,2)+"\n");
};
try{
 checkEnvironment();
 const identity=await request("/api/benchmark/identity");
 const d=realpathSync(join(root,"data")),w=realpathSync(join(root,"workspace"));
 if(identity.benchmark!==true||!identity.instanceId
    ||identity.dataRoot!==d||identity.workspace!==w)
  throw Error("Connected server is not this exact isolated benchmark engine");
 receipt.environment={
  identity:{instanceId:identity.instanceId,dataRoot:d,workspace:w,build:identity.build},
  node:process.version,platform:process.platform,cpuCount:(await import("node:os")).cpus().length,
 };
 const modelInfo=await fetch("http://127.0.0.1:11434/api/show",{
  method:"POST",headers:{"content-type":"application/json"},
  body:JSON.stringify({model}),signal:AbortSignal.timeout(15000)
 }).then(r=>r.ok?r.json():null).catch(()=>null);
 const tags=await fetch("http://127.0.0.1:11434/api/tags",{
  signal:AbortSignal.timeout(15000)
 }).then(r=>r.ok?r.json():null).catch(()=>null);
 receipt.environment.modelDigest=tags?.models?.find(m=>m.name===model||m.model===model)?.digest||null;
 receipt.environment.quantization=modelInfo?.details?.quantization_level||null;
 await request("/api/providers",{
  id:providerId,name:"Sub1 isolated live goal benchmark",
  kind:"openai",enabled:true,toolMode:"native",
  baseUrl:"http://127.0.0.1:11434/v1",model,
 });
 await request("/api/policy",{
  autoApproveAll:false,autoApproveMcp:false,
  autoApprove:["workspace_list","workspace_read","workspace_read_range",
   "workspace_write","workspace_replace","workspace_replace_range",
   "workspace_replace_function","code_search","file_read","file_write"]
 });
 persist();
 for(const task of cases){
  const result=await launchCase(task);
  receipt.cases.push(result);
  persist();
  process.stdout.write("MEASURED "+JSON.stringify({
   id:result.caseId,oraclePass:result.oraclePass,
   engineVerified:result.engineVerified,status:result.status,usage:result.usage
  })+"\n");
 }
 const passed=receipt.cases.filter(c=>c.oraclePass).length;
 const verified=receipt.cases.filter(c=>c.engineVerified).length;
 receipt.summary={
  totalCases:cases.length,oraclePasses:passed,
  engineVerifiedCompletions:verified,
  bothChecksPass:receipt.cases.filter(c=>c.oraclePass&&c.engineVerified).length,
  oraclePassRate:passed/cases.length,
  measuredTotalTokens:receipt.cases.reduce((sum,c)=>sum+(c.usage?.totalTokens||0),0),
  tokensEstimatedCalls:receipt.cases.reduce((sum,c)=>sum+(c.usage?.estimatedCalls||0),0),
  measuredTotalModelCalls:receipt.cases.reduce((sum,c)=>sum+(c.usage?.calls||0),0),
  billingKnown:false,qualityGeneralizationProven:false
 };
 receipt.completedAt=new Date().toISOString();
 persist();
 if(process.env.GITHUB_STEP_SUMMARY){
  appendFileSync(process.env.GITHUB_STEP_SUMMARY,
   "### Sub1 observed local-model goal outcomes\n"+
   "Real model: "+model+"; revision: "+receipt.revision+"\n\n"+
   "Private-oracle passes: **"+passed+"/"+cases.length+
   "**. Both model-goal proof and oracle pass: **"+receipt.summary.bothChecksPass+"/"+cases.length+
   "**. Observed tokens: "+receipt.summary.measuredTotalTokens+
   ". Provider billing: **not measured**. Task scope: 2 small multi-file fixtures (not SWE-bench).\n");
 }
}catch(error){
 receipt.error=String(error);receipt.completedAt=new Date().toISOString();persist();
 console.error("SUB1_LIVE_EVAL_ERROR",receipt.error);
 process.exitCode=1;
}
